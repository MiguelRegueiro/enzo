#include "internal.h"

#include <libavcodec/avcodec.h>
#include <libswscale/swscale.h>
#include <limits.h>
#include <math.h>
#include <stdlib.h>

struct EnzoVideoDecoder {
    AVFormatContext *format;
    AVCodecContext *codec;
    AVPacket *packet;
    AVFrame *frame;
    struct SwsContext *sws;
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
};

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

    decoder->codec = avcodec_alloc_context3(codec);
    if (decoder->codec == NULL) {
        enzo_set_error(err, err_len, "failed to allocate video codec context");
        enzo_video_decoder_close(decoder);
        return -1;
    }

    ret = avcodec_parameters_to_context(decoder->codec, stream->codecpar);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to copy video codec parameters", ret);
        enzo_video_decoder_close(decoder);
        return -1;
    }

    decoder->codec->thread_count = 0;
    ret = avcodec_open2(decoder->codec, codec, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open video decoder", ret);
        enzo_video_decoder_close(decoder);
        return -1;
    }

    decoder->packet = av_packet_alloc();
    decoder->frame = av_frame_alloc();
    if (decoder->packet == NULL || decoder->frame == NULL) {
        enzo_set_error(err, err_len, "failed to allocate video packet/frame");
        enzo_video_decoder_close(decoder);
        return -1;
    }

    decoder->sws = sws_getContext(
        decoder->codec->width,
        decoder->codec->height,
        decoder->codec->pix_fmt,
        out_width,
        out_height,
        AV_PIX_FMT_RGB24,
        SWS_FAST_BILINEAR,
        NULL,
        NULL,
        NULL
    );
    if (decoder->sws == NULL) {
        enzo_set_error(err, err_len, "failed to allocate video scaler");
        enzo_video_decoder_close(decoder);
        return -1;
    }

    *out = decoder;
    return 0;
}

static int receive_video_frame(
    EnzoVideoDecoder *decoder,
    uint8_t *rgb_out,
    double *pts_out,
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

        uint8_t *dst_data[4] = {rgb_out, NULL, NULL, NULL};
        int dst_linesize[4] = {decoder->out_width * 3, 0, 0, 0};
        sws_scale(
            decoder->sws,
            (const uint8_t *const *)decoder->frame->data,
            decoder->frame->linesize,
            0,
            decoder->codec->height,
            dst_data,
            dst_linesize
        );

        av_frame_unref(decoder->frame);
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
        int status = receive_video_frame(decoder, rgb_out, pts_out, err, err_len);
        if (status == 1 || status == 0 || status == -1) {
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
    if (decoder->frame != NULL) {
        av_frame_free(&decoder->frame);
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
