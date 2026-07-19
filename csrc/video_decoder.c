#include "internal.h"

#include <libavcodec/avcodec.h>
#include <libavfilter/avfilter.h>
#include <libavfilter/buffersink.h>
#include <libavfilter/buffersrc.h>
#include <libavutil/error.h>
#include <libavutil/hwcontext.h>
#include <libavutil/mem.h>
#include <libavutil/pixdesc.h>
#include <libswscale/swscale.h>
#include <limits.h>
#include <math.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

struct EnzoVideoDecoder {
    AVFormatContext *format;
    AVCodecContext *codec;
    AVPacket *packet;
    AVFrame *frame;
    AVFrame *software_frame;
    AVFrame *filtered_frame;
    struct SwsContext *sws;
    AVFilterGraph *filter_graph;
    AVFilterContext *filter_source;
    AVFilterContext *filter_sink;
    enum AVPixelFormat hw_pix_fmt;
    int stream_index;
    AVRational time_base;
    int64_t timestamp_origin;
    int out_width;
    int out_height;
    size_t frame_bytes;
    int flushing;
    int64_t frame_index;
    double fallback_interval;
    int has_seek_target;
    double seek_target;
    int filter_src_width;
    int filter_src_height;
    enum AVPixelFormat filter_src_format;
    int hw_filter_disabled;
};

static int hwaccel_disabled(void) {
    const char *disabled = getenv("ENZO_DISABLE_HWACCEL");
    if (disabled != NULL && disabled[0] != '\0' && strcmp(disabled, "0") != 0) {
        return 1;
    }

    const char *enabled = getenv("ENZO_ENABLE_HWACCEL");
    if (enabled != NULL && enabled[0] != '\0' && strcmp(enabled, "0") != 0) {
        return 0;
    }

#ifdef __FreeBSD__
    return 0;
#else
    return 1;
#endif
}

static int hw_filter_debug_enabled(void) {
    const char *value = getenv("ENZO_DEBUG_HW_FILTER");
    return value != NULL && value[0] != '\0' && strcmp(value, "0") != 0;
}

static void debug_hw_filter(const char *fmt, ...) {
    if (!hw_filter_debug_enabled()) {
        return;
    }

    fprintf(stderr, "enzo: vaapi filter: ");
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
    fprintf(stderr, "\n");
}

static void debug_hw_filter_error(const char *stage, int ret) {
    if (!hw_filter_debug_enabled()) {
        return;
    }

    char detail[AV_ERROR_MAX_STRING_SIZE] = {0};
    av_strerror(ret, detail, sizeof(detail));
    debug_hw_filter("%s failed: %s (%d)", stage, detail, ret);
}

static int codec_vaapi_format(
    const AVCodec *codec,
    enum AVPixelFormat *pix_fmt_out
) {
    for (int i = 0;; i++) {
        const AVCodecHWConfig *config = avcodec_get_hw_config(codec, i);
        if (config == NULL) {
            return 0;
        }
        if (
            (config->methods & AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX) != 0 &&
            config->device_type == AV_HWDEVICE_TYPE_VAAPI
        ) {
            *pix_fmt_out = config->pix_fmt;
            return 1;
        }
    }
}

static enum AVPixelFormat video_get_format(
    AVCodecContext *codec,
    const enum AVPixelFormat *pix_fmts
) {
    EnzoVideoDecoder *decoder = codec->opaque;
    if (decoder != NULL && decoder->hw_pix_fmt != AV_PIX_FMT_NONE) {
        for (const enum AVPixelFormat *fmt = pix_fmts; *fmt != AV_PIX_FMT_NONE; fmt++) {
            if (*fmt == decoder->hw_pix_fmt) {
                return *fmt;
            }
        }
    }

    return pix_fmts[0];
}

static const char *vaapi_device_path(void) {
    const char *configured = getenv("ENZO_VAAPI_DEVICE");
    if (configured != NULL && configured[0] != '\0') {
        return configured;
    }
    if (access("/dev/dri/renderD128", R_OK | W_OK) == 0) {
        return "/dev/dri/renderD128";
    }
    return NULL;
}

static int try_enable_vaapi(
    EnzoVideoDecoder *decoder,
    const AVCodec *codec,
    char *err,
    size_t err_len
) {
    if (hwaccel_disabled()) {
        return 0;
    }

    enum AVPixelFormat hw_pix_fmt = AV_PIX_FMT_NONE;
    if (!codec_vaapi_format(codec, &hw_pix_fmt)) {
        return 0;
    }

    AVBufferRef *device = NULL;
    const char *device_path = vaapi_device_path();
    int ret = av_hwdevice_ctx_create(
        &device,
        AV_HWDEVICE_TYPE_VAAPI,
        device_path,
        NULL,
        0
    );
    if (ret < 0 && device_path != NULL) {
        ret = av_hwdevice_ctx_create(
            &device,
            AV_HWDEVICE_TYPE_VAAPI,
            NULL,
            NULL,
            0
        );
    }
    if (ret < 0) {
        return 0;
    }

    decoder->codec->hw_device_ctx = av_buffer_ref(device);
    av_buffer_unref(&device);
    if (decoder->codec->hw_device_ctx == NULL) {
        enzo_set_error(err, err_len, "failed to reference VAAPI device");
        return -1;
    }

    decoder->hw_pix_fmt = hw_pix_fmt;
    decoder->codec->opaque = decoder;
    decoder->codec->get_format = video_get_format;
    return 1;
}

static int configure_video_codec(
    EnzoVideoDecoder *decoder,
    AVStream *stream,
    const AVCodec *codec,
    int allow_hwaccel,
    char *err,
    size_t err_len
) {
    decoder->hw_pix_fmt = AV_PIX_FMT_NONE;
    decoder->codec = avcodec_alloc_context3(codec);
    if (decoder->codec == NULL) {
        enzo_set_error(err, err_len, "failed to allocate video codec context");
        return -1;
    }

    int ret = avcodec_parameters_to_context(decoder->codec, stream->codecpar);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to copy video codec parameters", ret);
        avcodec_free_context(&decoder->codec);
        return -1;
    }

    decoder->codec->thread_count = 0;
    if (allow_hwaccel) {
        ret = try_enable_vaapi(decoder, codec, err, err_len);
        if (ret < 0) {
            avcodec_free_context(&decoder->codec);
            return -1;
        }
    }

    return 0;
}

static int open_video_codec(
    EnzoVideoDecoder *decoder,
    AVStream *stream,
    const AVCodec *codec,
    char *err,
    size_t err_len
) {
    if (configure_video_codec(decoder, stream, codec, 1, err, err_len) < 0) {
        return -1;
    }

    int ret = avcodec_open2(decoder->codec, codec, NULL);
    if (ret >= 0) {
        return 0;
    }

    if (decoder->hw_pix_fmt == AV_PIX_FMT_NONE) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open video decoder", ret);
        return -1;
    }

    avcodec_free_context(&decoder->codec);
    if (configure_video_codec(decoder, stream, codec, 0, err, err_len) < 0) {
        return -1;
    }

    ret = avcodec_open2(decoder->codec, codec, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open video decoder", ret);
        return -1;
    }

    return 0;
}

static void close_hw_filter(EnzoVideoDecoder *decoder) {
    if (decoder == NULL) {
        return;
    }
    avfilter_graph_free(&decoder->filter_graph);
    decoder->filter_source = NULL;
    decoder->filter_sink = NULL;
    decoder->filter_src_width = 0;
    decoder->filter_src_height = 0;
    decoder->filter_src_format = AV_PIX_FMT_NONE;
}

int enzo_video_decoder_open(
    const char *path,
    int out_width,
    int out_height,
    double fps,
    EnzoVideoDecoder **out,
    char *err,
    size_t err_len
) {
    enzo_suppress_ffmpeg_logs();

    if (
        path == NULL ||
        out == NULL ||
        out_width <= 0 ||
        out_height <= 0 ||
        out_width > INT_MAX / 3
    ) {
        enzo_set_error(err, err_len, "invalid video decoder arguments");
        return -1;
    }
    size_t width = (size_t)out_width;
    size_t height = (size_t)out_height;
    if (height > SIZE_MAX / width) {
        enzo_set_error(err, err_len, "video frame dimensions are too large");
        return -1;
    }
    size_t pixels = width * height;
    if (pixels > SIZE_MAX / 3) {
        enzo_set_error(err, err_len, "video frame buffer is too large");
        return -1;
    }

    EnzoVideoDecoder *decoder = calloc(1, sizeof(EnzoVideoDecoder));
    if (decoder == NULL) {
        enzo_set_error(err, err_len, "failed to allocate video decoder");
        return -1;
    }
    decoder->hw_pix_fmt = AV_PIX_FMT_NONE;
    decoder->out_width = out_width;
    decoder->out_height = out_height;
    decoder->frame_bytes = pixels * 3;
    decoder->fallback_interval =
        1.0 / (isfinite(fps) && fps > 0.0 ? fps : 30.0);

    int ret = avformat_open_input(&decoder->format, path, NULL, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open input", ret);
        enzo_video_decoder_close(decoder);
        return -1;
    }

    ret = avformat_find_stream_info(decoder->format, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to read stream info", ret);
        enzo_video_decoder_close(decoder);
        return -1;
    }

    decoder->stream_index =
        av_find_best_stream(decoder->format, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (decoder->stream_index < 0) {
        enzo_set_error(err, err_len, "input has no video stream");
        enzo_video_decoder_close(decoder);
        return -1;
    }

    AVStream *stream = decoder->format->streams[decoder->stream_index];
    decoder->time_base = stream->time_base;
    decoder->timestamp_origin =
        enzo_stream_timestamp_origin(decoder->format, stream);
    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (codec == NULL) {
        enzo_set_error(err, err_len, "failed to find video decoder");
        enzo_video_decoder_close(decoder);
        return -1;
    }

    if (open_video_codec(decoder, stream, codec, err, err_len) < 0) {
        enzo_video_decoder_close(decoder);
        return -1;
    }

    decoder->packet = av_packet_alloc();
    decoder->frame = av_frame_alloc();
    decoder->software_frame = av_frame_alloc();
    decoder->filtered_frame = av_frame_alloc();
    if (
        decoder->packet == NULL ||
        decoder->frame == NULL ||
        decoder->software_frame == NULL ||
        decoder->filtered_frame == NULL
    ) {
        enzo_set_error(err, err_len, "failed to allocate video packet/frame");
        enzo_video_decoder_close(decoder);
        return -1;
    }

    *out = decoder;
    return 0;
}

static AVRational valid_sample_aspect_ratio(const AVFrame *frame) {
    AVRational ratio = frame->sample_aspect_ratio;
    if (ratio.num <= 0 || ratio.den <= 0) {
        ratio = (AVRational){1, 1};
    }
    return ratio;
}

static int configure_vaapi_filter(
    EnzoVideoDecoder *decoder,
    const AVFrame *frame,
    enum AVPixelFormat output_format
) {
    if (frame->hw_frames_ctx == NULL) {
        debug_hw_filter("source frame has no hw_frames_ctx");
        return -1;
    }
    const char *output_format_name = av_get_pix_fmt_name(output_format);
    if (output_format_name == NULL) {
        debug_hw_filter("unknown output pixel format %d", output_format);
        return -1;
    }

    close_hw_filter(decoder);

    AVFilterGraph *graph = avfilter_graph_alloc();
    if (graph == NULL) {
        debug_hw_filter("failed to allocate filter graph");
        return -1;
    }

    const AVFilter *buffer_filter = avfilter_get_by_name("buffer");
    const AVFilter *scale_filter = avfilter_get_by_name("scale_vaapi");
    const AVFilter *download_filter = avfilter_get_by_name("hwdownload");
    const AVFilter *format_filter = avfilter_get_by_name("format");
    const AVFilter *sink_filter = avfilter_get_by_name("buffersink");
    if (
        buffer_filter == NULL ||
        scale_filter == NULL ||
        download_filter == NULL ||
        format_filter == NULL ||
        sink_filter == NULL
    ) {
        debug_hw_filter("required filter is unavailable");
        avfilter_graph_free(&graph);
        return -1;
    }

    AVFilterContext *source = NULL;
    AVFilterContext *scale = NULL;
    AVFilterContext *download = NULL;
    AVFilterContext *format = NULL;
    AVFilterContext *sink = NULL;
    int ret = 0;
    source = avfilter_graph_alloc_filter(graph, buffer_filter, "video_in");
    if (source == NULL) {
        debug_hw_filter("failed to allocate buffer source");
        avfilter_graph_free(&graph);
        return -1;
    }

    AVBufferSrcParameters *params = av_buffersrc_parameters_alloc();
    if (params == NULL) {
        debug_hw_filter("failed to allocate buffer source parameters");
        avfilter_graph_free(&graph);
        return -1;
    }
    AVRational aspect = valid_sample_aspect_ratio(frame);
    params->format = frame->format;
    params->width = frame->width;
    params->height = frame->height;
    params->time_base = decoder->time_base;
    params->sample_aspect_ratio = aspect;
    params->hw_frames_ctx = av_buffer_ref(frame->hw_frames_ctx);
    if (params->hw_frames_ctx == NULL) {
        debug_hw_filter("failed to reference hw_frames_ctx");
        av_free(params);
        avfilter_graph_free(&graph);
        return -1;
    }
    ret = av_buffersrc_parameters_set(source, params);
    av_buffer_unref(&params->hw_frames_ctx);
    av_free(params);
    if (ret < 0) {
        debug_hw_filter_error("set buffer source parameters", ret);
        avfilter_graph_free(&graph);
        return -1;
    }
    ret = avfilter_init_str(source, NULL);
    if (ret < 0) {
        debug_hw_filter_error("initialize buffer source", ret);
        avfilter_graph_free(&graph);
        return -1;
    }

    char scale_args[128];
    int written = snprintf(
        scale_args,
        sizeof(scale_args),
        "w=%d:h=%d:format=%s",
        decoder->out_width,
        decoder->out_height,
        output_format_name
    );
    if (written < 0 || (size_t)written >= sizeof(scale_args)) {
        debug_hw_filter("scale args are too long");
        avfilter_graph_free(&graph);
        return -1;
    }

    ret = avfilter_graph_create_filter(
        &scale,
        scale_filter,
        "vaapi_scale",
        scale_args,
        NULL,
        graph
    );
    if (ret < 0) {
        debug_hw_filter_error("create scale_vaapi", ret);
        avfilter_graph_free(&graph);
        return -1;
    }
    ret = avfilter_graph_create_filter(
        &download,
        download_filter,
        "vaapi_download",
        NULL,
        NULL,
        graph
    );
    if (ret < 0) {
        debug_hw_filter_error("create hwdownload", ret);
        avfilter_graph_free(&graph);
        return -1;
    }

    char format_args[64];
    written = snprintf(
        format_args,
        sizeof(format_args),
        "pix_fmts=%s",
        output_format_name
    );
    if (written < 0 || (size_t)written >= sizeof(format_args)) {
        debug_hw_filter("format args are too long");
        avfilter_graph_free(&graph);
        return -1;
    }
    ret = avfilter_graph_create_filter(
        &format,
        format_filter,
        "download_format",
        format_args,
        NULL,
        graph
    );
    if (ret < 0) {
        debug_hw_filter_error("create download format", ret);
        avfilter_graph_free(&graph);
        return -1;
    }
    ret = avfilter_graph_create_filter(
        &sink,
        sink_filter,
        "video_out",
        NULL,
        NULL,
        graph
    );
    if (ret < 0) {
        debug_hw_filter_error("create buffer sink", ret);
        avfilter_graph_free(&graph);
        return -1;
    }

    ret = avfilter_link(source, 0, scale, 0);
    if (ret < 0) {
        debug_hw_filter_error("link source to scale_vaapi", ret);
    }
    if (ret >= 0) {
        ret = avfilter_link(scale, 0, download, 0);
        if (ret < 0) {
            debug_hw_filter_error("link scale_vaapi to hwdownload", ret);
        }
    }
    if (ret >= 0) {
        ret = avfilter_link(download, 0, format, 0);
        if (ret < 0) {
            debug_hw_filter_error("link hwdownload to format", ret);
        }
    }
    if (ret >= 0) {
        ret = avfilter_link(format, 0, sink, 0);
        if (ret < 0) {
            debug_hw_filter_error("link format to sink", ret);
        }
    }
    if (ret >= 0) {
        ret = avfilter_graph_config(graph, NULL);
        if (ret < 0) {
            debug_hw_filter_error("configure graph", ret);
        }
    }
    if (ret < 0) {
        avfilter_graph_free(&graph);
        return -1;
    }

    decoder->filter_graph = graph;
    decoder->filter_source = source;
    decoder->filter_sink = sink;
    decoder->filter_src_width = frame->width;
    decoder->filter_src_height = frame->height;
    decoder->filter_src_format = (enum AVPixelFormat)frame->format;
    debug_hw_filter(
        "configured %dx%d -> %dx%d %s",
        frame->width,
        frame->height,
        decoder->out_width,
        decoder->out_height,
        output_format_name
    );
    return 0;
}

static int configure_best_vaapi_filter(
    EnzoVideoDecoder *decoder,
    const AVFrame *frame
) {
    if (configure_vaapi_filter(decoder, frame, AV_PIX_FMT_RGB0) == 0) {
        return 0;
    }
    return configure_vaapi_filter(decoder, frame, AV_PIX_FMT_NV12);
}

static const AVFrame *filter_vaapi_frame(EnzoVideoDecoder *decoder) {
    if (decoder->hw_filter_disabled) {
        return NULL;
    }
    if (decoder->hw_pix_fmt == AV_PIX_FMT_NONE) {
        return NULL;
    }
    if (decoder->frame->format != decoder->hw_pix_fmt) {
        debug_hw_filter(
            "skipping non-hardware frame format %d, expected %d",
            decoder->frame->format,
            decoder->hw_pix_fmt
        );
        return NULL;
    }

    if (
        decoder->filter_graph == NULL ||
        decoder->filter_src_width != decoder->frame->width ||
        decoder->filter_src_height != decoder->frame->height ||
        decoder->filter_src_format != (enum AVPixelFormat)decoder->frame->format
    ) {
        if (configure_best_vaapi_filter(decoder, decoder->frame) < 0) {
            decoder->hw_filter_disabled = 1;
            close_hw_filter(decoder);
            return NULL;
        }
    }

    av_frame_unref(decoder->filtered_frame);
    int ret = av_buffersrc_add_frame_flags(
        decoder->filter_source,
        decoder->frame,
        AV_BUFFERSRC_FLAG_KEEP_REF
    );
    if (ret < 0) {
        debug_hw_filter_error("add frame to graph", ret);
        decoder->hw_filter_disabled = 1;
        close_hw_filter(decoder);
        return NULL;
    }

    ret = av_buffersink_get_frame(decoder->filter_sink, decoder->filtered_frame);
    if (ret < 0) {
        debug_hw_filter_error("get filtered frame", ret);
        decoder->hw_filter_disabled = 1;
        close_hw_filter(decoder);
        av_frame_unref(decoder->filtered_frame);
        return NULL;
    }

    return decoder->filtered_frame;
}

static const AVFrame *download_video_frame(
    EnzoVideoDecoder *decoder,
    char *err,
    size_t err_len
) {
    if (
        decoder->hw_pix_fmt == AV_PIX_FMT_NONE ||
        decoder->frame->format != decoder->hw_pix_fmt
    ) {
        return decoder->frame;
    }

    av_frame_unref(decoder->software_frame);
    int ret = av_hwframe_transfer_data(
        decoder->software_frame,
        decoder->frame,
        0
    );
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to download hardware video frame", ret);
        return NULL;
    }

    return decoder->software_frame;
}

static int copy_rgb0_video_frame(
    EnzoVideoDecoder *decoder,
    const AVFrame *frame,
    uint8_t *rgb_out,
    char *err,
    size_t err_len
) {
    if (
        frame->format != AV_PIX_FMT_RGB0 ||
        frame->width != decoder->out_width ||
        frame->height != decoder->out_height
    ) {
        return 0;
    }
    size_t min_src_stride = (size_t)decoder->out_width * 4;
    if (
        frame->data[0] == NULL ||
        frame->linesize[0] < 0 ||
        (size_t)frame->linesize[0] < min_src_stride
    ) {
        enzo_set_error(err, err_len, "decoded RGB0 video frame has invalid layout");
        return -1;
    }

    const size_t src_stride = (size_t)frame->linesize[0];
    const size_t dst_stride = (size_t)decoder->out_width * 3;
    for (int y = 0; y < decoder->out_height; y++) {
        const uint8_t *src = frame->data[0] + (size_t)y * src_stride;
        uint8_t *dst = rgb_out + (size_t)y * dst_stride;
        for (int x = 0; x < decoder->out_width; x++) {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2] = src[2];
            src += 4;
            dst += 3;
        }
    }

    return 1;
}

static int scale_video_frame(
    EnzoVideoDecoder *decoder,
    const AVFrame *frame,
    uint8_t *rgb_out,
    char *err,
    size_t err_len
) {
    if (
        frame == NULL ||
        frame->width <= 0 ||
        frame->height <= 0 ||
        frame->format == AV_PIX_FMT_NONE
    ) {
        enzo_set_error(err, err_len, "decoded video frame has invalid dimensions or format");
        return -1;
    }

    int copied = copy_rgb0_video_frame(decoder, frame, rgb_out, err, err_len);
    if (copied != 0) {
        return copied < 0 ? -1 : 0;
    }

    decoder->sws = sws_getCachedContext(
        decoder->sws,
        frame->width,
        frame->height,
        (enum AVPixelFormat)frame->format,
        decoder->out_width,
        decoder->out_height,
        AV_PIX_FMT_RGB24,
        SWS_FAST_BILINEAR,
        NULL,
        NULL,
        NULL
    );
    if (decoder->sws == NULL) {
        enzo_set_error(err, err_len, "failed to allocate video scaler");
        return -1;
    }

    uint8_t *dst_data[4] = {rgb_out, NULL, NULL, NULL};
    int dst_linesize[4] = {decoder->out_width * 3, 0, 0, 0};
    int scaled_rows = sws_scale(
        decoder->sws,
        (const uint8_t *const *)frame->data,
        frame->linesize,
        0,
        frame->height,
        dst_data,
        dst_linesize
    );
    if (scaled_rows <= 0) {
        enzo_set_error(err, err_len, "failed to scale video frame");
        return -1;
    }

    return 0;
}

static int receive_video_frame(
    EnzoVideoDecoder *decoder,
    uint8_t *rgb_out,
    double *pts_out,
    double drop_before_pts,
    char *err,
    size_t err_len
) {
    int ret = avcodec_receive_frame(decoder->codec, decoder->frame);
    if (ret == 0) {
        int64_t timestamp = decoder->frame->best_effort_timestamp;
        if (timestamp != AV_NOPTS_VALUE) {
            *pts_out =
                (double)(timestamp - decoder->timestamp_origin) *
                av_q2d(decoder->time_base);
        } else {
            *pts_out = (double)decoder->frame_index * decoder->fallback_interval;
        }
        decoder->frame_index++;

        if (decoder->has_seek_target && *pts_out + 0.050 < decoder->seek_target) {
            av_frame_unref(decoder->frame);
            return 2;
        }
        decoder->has_seek_target = 0;

        if (isfinite(drop_before_pts) && *pts_out < drop_before_pts) {
            av_frame_unref(decoder->frame);
            return 3;
        }

        const AVFrame *scale_frame = filter_vaapi_frame(decoder);
        if (scale_frame == NULL) {
            scale_frame = download_video_frame(decoder, err, err_len);
        }
        if (scale_frame == NULL) {
            av_frame_unref(decoder->frame);
            return -1;
        }
        if (scale_video_frame(decoder, scale_frame, rgb_out, err, err_len) < 0) {
            av_frame_unref(decoder->frame);
            av_frame_unref(decoder->software_frame);
            return -1;
        }

        av_frame_unref(decoder->frame);
        av_frame_unref(decoder->software_frame);
        av_frame_unref(decoder->filtered_frame);
        return 1;
    }

    if (ret == AVERROR(EAGAIN)) {
        return 2;
    }
    if (ret == AVERROR_EOF) {
        return 0;
    }

    enzo_set_ffmpeg_error(err, err_len, "failed to receive video frame", ret);
    return -1;
}

int enzo_video_decoder_next(
    EnzoVideoDecoder *decoder,
    uint8_t *rgb_out,
    size_t rgb_len,
    double *pts_out,
    double drop_before_pts,
    const int *stop_flag,
    const int *seek_generation,
    int expected_seek_generation,
    char *err,
    size_t err_len
) {
    if (decoder == NULL || rgb_out == NULL || pts_out == NULL) {
        enzo_set_error(err, err_len, "invalid video frame arguments");
        return -1;
    }
    if (rgb_len < decoder->frame_bytes) {
        enzo_set_error(
            err,
            err_len,
            "video frame buffer is too small: need %zu bytes, received %zu",
            decoder->frame_bytes,
            rgb_len
        );
        return -1;
    }

    while (!enzo_stop_requested(stop_flag)) {
        if (enzo_seek_generation_value(seek_generation) != expected_seek_generation) {
            return 2;
        }
        int status =
            receive_video_frame(decoder, rgb_out, pts_out, drop_before_pts, err, err_len);
        if (status == 1 || status == 0 || status == -1 || status == 3) {
            return status;
        }
        if (enzo_seek_generation_value(seek_generation) != expected_seek_generation) {
            return 2;
        }

        if (decoder->flushing) {
            return 0;
        }

        int ret = av_read_frame(decoder->format, decoder->packet);
        if (ret == AVERROR_EOF) {
            decoder->flushing = 1;
            ret = avcodec_send_packet(decoder->codec, NULL);
            if (ret < 0 && ret != AVERROR_EOF) {
                enzo_set_ffmpeg_error(err, err_len, "failed to flush video decoder", ret);
                return -1;
            }
            continue;
        }
        if (ret < 0) {
            enzo_set_ffmpeg_error(err, err_len, "failed to read video packet", ret);
            return -1;
        }

        if (decoder->packet->stream_index == decoder->stream_index) {
            ret = avcodec_send_packet(decoder->codec, decoder->packet);
            av_packet_unref(decoder->packet);
            if (ret < 0 && ret != AVERROR(EAGAIN)) {
                enzo_set_ffmpeg_error(err, err_len, "failed to send video packet", ret);
                return -1;
            }
        } else {
            av_packet_unref(decoder->packet);
        }
    }

    return 0;
}

int enzo_video_decoder_seek(
    EnzoVideoDecoder *decoder,
    double seconds,
    int exact,
    char *err,
    size_t err_len
) {
    if (decoder == NULL) {
        enzo_set_error(err, err_len, "invalid video seek arguments");
        return -1;
    }

    if (!isfinite(seconds) || seconds < 0.0) {
        seconds = 0.0;
    }

    AVStream *stream = decoder->format->streams[decoder->stream_index];
    int64_t timestamp = av_rescale_q(
        (int64_t)(seconds * (double)AV_TIME_BASE),
        AV_TIME_BASE_Q,
        stream->time_base
    ) + decoder->timestamp_origin;
    int ret = av_seek_frame(
        decoder->format,
        decoder->stream_index,
        timestamp,
        AVSEEK_FLAG_BACKWARD
    );
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to seek video", ret);
        return -1;
    }

    avcodec_flush_buffers(decoder->codec);
    close_hw_filter(decoder);
    decoder->flushing = 0;
    decoder->frame_index = (int64_t)(seconds / decoder->fallback_interval);
    decoder->seek_target = seconds;
    decoder->has_seek_target = exact != 0;
    return 0;
}

void enzo_video_decoder_close(EnzoVideoDecoder *decoder) {
    if (decoder == NULL) {
        return;
    }
    if (decoder->sws != NULL) {
        sws_freeContext(decoder->sws);
    }
    close_hw_filter(decoder);
    if (decoder->frame != NULL) {
        av_frame_free(&decoder->frame);
    }
    if (decoder->software_frame != NULL) {
        av_frame_free(&decoder->software_frame);
    }
    if (decoder->filtered_frame != NULL) {
        av_frame_free(&decoder->filtered_frame);
    }
    if (decoder->packet != NULL) {
        av_packet_free(&decoder->packet);
    }
    if (decoder->codec != NULL) {
        avcodec_free_context(&decoder->codec);
    }
    if (decoder->format != NULL) {
        avformat_close_input(&decoder->format);
    }
    free(decoder);
}
