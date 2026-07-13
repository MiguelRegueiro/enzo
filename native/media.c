#include <errno.h>
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/channel_layout.h>
#include <libavutil/error.h>
#include <libavutil/imgutils.h>
#include <libavutil/mathematics.h>
#include <libavutil/opt.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>
#include <pulse/error.h>
#include <pulse/sample.h>
#include <pulse/simple.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct RigVideoInfo {
    uint32_t width;
    uint32_t height;
    double fps;
    int has_audio;
} RigVideoInfo;

typedef struct RigVideoDecoder {
    AVFormatContext *format;
    AVCodecContext *codec;
    AVPacket *packet;
    AVFrame *frame;
    struct SwsContext *sws;
    int stream_index;
    AVRational time_base;
    int out_width;
    int out_height;
    int flushing;
    int64_t frame_index;
    double fallback_interval;
} RigVideoDecoder;

void rig_video_decoder_close(RigVideoDecoder *decoder);

static int stop_requested(const int *stop_flag) {
    return stop_flag != NULL && *((volatile const int *)stop_flag) != 0;
}

static void set_error(char *err, size_t err_len, const char *fmt, ...) {
    if (err == NULL || err_len == 0) {
        return;
    }

    va_list args;
    va_start(args, fmt);
    vsnprintf(err, err_len, fmt, args);
    va_end(args);
}

static void set_ffmpeg_error(char *err, size_t err_len, const char *prefix, int code) {
    char detail[AV_ERROR_MAX_STRING_SIZE] = {0};
    av_strerror(code, detail, sizeof(detail));
    set_error(err, err_len, "%s: %s", prefix, detail);
}

static double rational_to_fps(AVRational value) {
    if (value.num <= 0 || value.den <= 0) {
        return 0.0;
    }
    return (double)value.num / (double)value.den;
}

static double stream_fps(const AVStream *stream) {
    double fps = rational_to_fps(stream->avg_frame_rate);
    if (fps <= 0.0) {
        fps = rational_to_fps(stream->r_frame_rate);
    }
    if (fps <= 0.0) {
        fps = 30.0;
    }
    if (fps > 30.0) {
        fps = 30.0;
    }
    return fps;
}

int rig_probe_video(const char *path, RigVideoInfo *out, char *err, size_t err_len) {
    if (path == NULL || out == NULL) {
        set_error(err, err_len, "invalid probe arguments");
        return -1;
    }

    AVFormatContext *format = NULL;
    int ret = avformat_open_input(&format, path, NULL, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to open input", ret);
        return -1;
    }

    ret = avformat_find_stream_info(format, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to read stream info", ret);
        avformat_close_input(&format);
        return -1;
    }

    int video_index = av_find_best_stream(format, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (video_index < 0) {
        set_error(err, err_len, "input has no video stream");
        avformat_close_input(&format);
        return -1;
    }

    AVStream *video = format->streams[video_index];
    out->width = (uint32_t)video->codecpar->width;
    out->height = (uint32_t)video->codecpar->height;
    out->fps = stream_fps(video);
    out->has_audio =
        av_find_best_stream(format, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0) >= 0;

    avformat_close_input(&format);
    return 0;
}

int rig_video_decoder_open(
    const char *path,
    int out_width,
    int out_height,
    double fps,
    RigVideoDecoder **out,
    char *err,
    size_t err_len
) {
    if (path == NULL || out == NULL || out_width <= 0 || out_height <= 0) {
        set_error(err, err_len, "invalid video decoder arguments");
        return -1;
    }

    RigVideoDecoder *decoder = calloc(1, sizeof(RigVideoDecoder));
    if (decoder == NULL) {
        set_error(err, err_len, "failed to allocate video decoder");
        return -1;
    }
    decoder->out_width = out_width;
    decoder->out_height = out_height;
    decoder->fallback_interval = 1.0 / (fps > 0.0 ? fps : 30.0);

    int ret = avformat_open_input(&decoder->format, path, NULL, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to open input", ret);
        rig_video_decoder_close(decoder);
        return -1;
    }

    ret = avformat_find_stream_info(decoder->format, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to read stream info", ret);
        rig_video_decoder_close(decoder);
        return -1;
    }

    decoder->stream_index =
        av_find_best_stream(decoder->format, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (decoder->stream_index < 0) {
        set_error(err, err_len, "input has no video stream");
        rig_video_decoder_close(decoder);
        return -1;
    }

    AVStream *stream = decoder->format->streams[decoder->stream_index];
    decoder->time_base = stream->time_base;
    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (codec == NULL) {
        set_error(err, err_len, "failed to find video decoder");
        rig_video_decoder_close(decoder);
        return -1;
    }

    decoder->codec = avcodec_alloc_context3(codec);
    if (decoder->codec == NULL) {
        set_error(err, err_len, "failed to allocate video codec context");
        rig_video_decoder_close(decoder);
        return -1;
    }

    ret = avcodec_parameters_to_context(decoder->codec, stream->codecpar);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to copy video codec parameters", ret);
        rig_video_decoder_close(decoder);
        return -1;
    }

    decoder->codec->thread_count = 0;
    ret = avcodec_open2(decoder->codec, codec, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to open video decoder", ret);
        rig_video_decoder_close(decoder);
        return -1;
    }

    decoder->packet = av_packet_alloc();
    decoder->frame = av_frame_alloc();
    if (decoder->packet == NULL || decoder->frame == NULL) {
        set_error(err, err_len, "failed to allocate video packet/frame");
        rig_video_decoder_close(decoder);
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
        set_error(err, err_len, "failed to allocate video scaler");
        rig_video_decoder_close(decoder);
        return -1;
    }

    *out = decoder;
    return 0;
}

static int receive_video_frame(
    RigVideoDecoder *decoder,
    uint8_t *rgb_out,
    double *pts_out,
    char *err,
    size_t err_len
) {
    int ret = avcodec_receive_frame(decoder->codec, decoder->frame);
    if (ret == 0) {
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

        int64_t timestamp = decoder->frame->best_effort_timestamp;
        if (timestamp != AV_NOPTS_VALUE) {
            *pts_out = (double)timestamp * av_q2d(decoder->time_base);
        } else {
            *pts_out = (double)decoder->frame_index * decoder->fallback_interval;
        }
        decoder->frame_index++;
        av_frame_unref(decoder->frame);
        return 1;
    }

    if (ret == AVERROR(EAGAIN)) {
        return 2;
    }
    if (ret == AVERROR_EOF) {
        return 0;
    }

    set_ffmpeg_error(err, err_len, "failed to receive video frame", ret);
    return -1;
}

int rig_video_decoder_next(
    RigVideoDecoder *decoder,
    uint8_t *rgb_out,
    double *pts_out,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
    if (decoder == NULL || rgb_out == NULL || pts_out == NULL) {
        set_error(err, err_len, "invalid video frame arguments");
        return -1;
    }

    while (!stop_requested(stop_flag)) {
        int status = receive_video_frame(decoder, rgb_out, pts_out, err, err_len);
        if (status == 1 || status == 0 || status == -1) {
            return status;
        }

        if (decoder->flushing) {
            return 0;
        }

        int ret = av_read_frame(decoder->format, decoder->packet);
        if (ret == AVERROR_EOF) {
            decoder->flushing = 1;
            ret = avcodec_send_packet(decoder->codec, NULL);
            if (ret < 0 && ret != AVERROR_EOF) {
                set_ffmpeg_error(err, err_len, "failed to flush video decoder", ret);
                return -1;
            }
            continue;
        }
        if (ret < 0) {
            set_ffmpeg_error(err, err_len, "failed to read video packet", ret);
            return -1;
        }

        if (decoder->packet->stream_index == decoder->stream_index) {
            ret = avcodec_send_packet(decoder->codec, decoder->packet);
            av_packet_unref(decoder->packet);
            if (ret < 0 && ret != AVERROR(EAGAIN)) {
                set_ffmpeg_error(err, err_len, "failed to send video packet", ret);
                return -1;
            }
        } else {
            av_packet_unref(decoder->packet);
        }
    }

    return 0;
}

void rig_video_decoder_close(RigVideoDecoder *decoder) {
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

static int open_audio_decoder(
    const char *path,
    AVFormatContext **format_out,
    AVCodecContext **codec_out,
    int *stream_index_out,
    char *err,
    size_t err_len
) {
    AVFormatContext *format = NULL;
    int ret = avformat_open_input(&format, path, NULL, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to open audio input", ret);
        return -1;
    }

    ret = avformat_find_stream_info(format, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to read audio stream info", ret);
        avformat_close_input(&format);
        return -1;
    }

    int stream_index = av_find_best_stream(format, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0);
    if (stream_index < 0) {
        avformat_close_input(&format);
        return 0;
    }

    AVStream *stream = format->streams[stream_index];
    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (codec == NULL) {
        set_error(err, err_len, "failed to find audio decoder");
        avformat_close_input(&format);
        return -1;
    }

    AVCodecContext *codec_context = avcodec_alloc_context3(codec);
    if (codec_context == NULL) {
        set_error(err, err_len, "failed to allocate audio codec context");
        avformat_close_input(&format);
        return -1;
    }

    ret = avcodec_parameters_to_context(codec_context, stream->codecpar);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to copy audio codec parameters", ret);
        avcodec_free_context(&codec_context);
        avformat_close_input(&format);
        return -1;
    }

    ret = avcodec_open2(codec_context, codec, NULL);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to open audio decoder", ret);
        avcodec_free_context(&codec_context);
        avformat_close_input(&format);
        return -1;
    }

    *format_out = format;
    *codec_out = codec_context;
    *stream_index_out = stream_index;
    return 1;
}

static int write_converted_audio(
    SwrContext *swr,
    AVCodecContext *codec,
    AVFrame *frame,
    pa_simple *pulse,
    uint8_t **out_buffer,
    int *out_capacity,
    char *err,
    size_t err_len
) {
    int out_samples = (int)av_rescale_rnd(
        swr_get_delay(swr, codec->sample_rate) + frame->nb_samples,
        48000,
        codec->sample_rate,
        AV_ROUND_UP
    );
    if (out_samples <= 0) {
        return 0;
    }

    if (out_samples > *out_capacity) {
        av_freep(out_buffer);
        int line_size = 0;
        int ret = av_samples_alloc(
            out_buffer,
            &line_size,
            2,
            out_samples,
            AV_SAMPLE_FMT_S16,
            0
        );
        if (ret < 0) {
            set_ffmpeg_error(err, err_len, "failed to allocate audio buffer", ret);
            return -1;
        }
        *out_capacity = out_samples;
    }

    uint8_t *output_planes[1] = {*out_buffer};
    int converted = swr_convert(
        swr,
        output_planes,
        out_samples,
        (const uint8_t **)frame->extended_data,
        frame->nb_samples
    );
    if (converted < 0) {
        set_ffmpeg_error(err, err_len, "failed to resample audio", converted);
        return -1;
    }

    int bytes = av_samples_get_buffer_size(NULL, 2, converted, AV_SAMPLE_FMT_S16, 1);
    if (bytes < 0) {
        set_ffmpeg_error(err, err_len, "failed to size audio buffer", bytes);
        return -1;
    }

    int pulse_error = 0;
    if (bytes > 0 && pa_simple_write(pulse, *out_buffer, (size_t)bytes, &pulse_error) < 0) {
        set_error(err, err_len, "failed to write audio: %s", pa_strerror(pulse_error));
        return -1;
    }

    return 0;
}

int rig_play_audio(const char *path, const int *stop_flag, char *err, size_t err_len) {
    if (path == NULL) {
        set_error(err, err_len, "invalid audio path");
        return -1;
    }

    AVFormatContext *format = NULL;
    AVCodecContext *codec = NULL;
    int stream_index = -1;
    int opened = open_audio_decoder(path, &format, &codec, &stream_index, err, err_len);
    if (opened <= 0) {
        return opened;
    }

    AVChannelLayout src_layout;
    if (codec->ch_layout.nb_channels > 0) {
        av_channel_layout_copy(&src_layout, &codec->ch_layout);
    } else {
        av_channel_layout_default(&src_layout, codec->ch_layout.nb_channels > 0
                                                   ? codec->ch_layout.nb_channels
                                                   : 2);
    }

    AVChannelLayout dst_layout;
    av_channel_layout_default(&dst_layout, 2);

    SwrContext *swr = NULL;
    int ret = swr_alloc_set_opts2(
        &swr,
        &dst_layout,
        AV_SAMPLE_FMT_S16,
        48000,
        &src_layout,
        codec->sample_fmt,
        codec->sample_rate,
        0,
        NULL
    );
    if (ret < 0 || swr == NULL) {
        set_ffmpeg_error(err, err_len, "failed to allocate audio resampler", ret);
        av_channel_layout_uninit(&src_layout);
        av_channel_layout_uninit(&dst_layout);
        avcodec_free_context(&codec);
        avformat_close_input(&format);
        return -1;
    }

    ret = swr_init(swr);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to initialize audio resampler", ret);
        swr_free(&swr);
        av_channel_layout_uninit(&src_layout);
        av_channel_layout_uninit(&dst_layout);
        avcodec_free_context(&codec);
        avformat_close_input(&format);
        return -1;
    }

    pa_sample_spec sample_spec = {
        .format = PA_SAMPLE_S16LE,
        .rate = 48000,
        .channels = 2,
    };
    int pulse_error = 0;
    pa_simple *pulse = pa_simple_new(
        NULL,
        "rigoberto",
        PA_STREAM_PLAYBACK,
        NULL,
        "playback",
        &sample_spec,
        NULL,
        NULL,
        &pulse_error
    );
    if (pulse == NULL) {
        set_error(err, err_len, "failed to open PulseAudio: %s", pa_strerror(pulse_error));
        swr_free(&swr);
        av_channel_layout_uninit(&src_layout);
        av_channel_layout_uninit(&dst_layout);
        avcodec_free_context(&codec);
        avformat_close_input(&format);
        return -1;
    }

    AVPacket *packet = av_packet_alloc();
    AVFrame *frame = av_frame_alloc();
    uint8_t *out_buffer = NULL;
    int out_capacity = 0;
    int failed = 0;
    int flushing = 0;

    if (packet == NULL || frame == NULL) {
        set_error(err, err_len, "failed to allocate audio packet/frame");
        failed = 1;
    }

    while (!failed && !stop_requested(stop_flag)) {
        ret = avcodec_receive_frame(codec, frame);
        if (ret == 0) {
            if (write_converted_audio(
                    swr,
                    codec,
                    frame,
                    pulse,
                    &out_buffer,
                    &out_capacity,
                    err,
                    err_len
                ) < 0) {
                failed = 1;
            }
            av_frame_unref(frame);
            continue;
        }
        if (ret == AVERROR_EOF) {
            break;
        }
        if (ret != AVERROR(EAGAIN)) {
            set_ffmpeg_error(err, err_len, "failed to receive audio frame", ret);
            failed = 1;
            break;
        }

        if (flushing) {
            break;
        }

        ret = av_read_frame(format, packet);
        if (ret == AVERROR_EOF) {
            flushing = 1;
            ret = avcodec_send_packet(codec, NULL);
            if (ret < 0 && ret != AVERROR_EOF) {
                set_ffmpeg_error(err, err_len, "failed to flush audio decoder", ret);
                failed = 1;
            }
            continue;
        }
        if (ret < 0) {
            set_ffmpeg_error(err, err_len, "failed to read audio packet", ret);
            failed = 1;
            break;
        }

        if (packet->stream_index == stream_index) {
            ret = avcodec_send_packet(codec, packet);
            av_packet_unref(packet);
            if (ret < 0 && ret != AVERROR(EAGAIN)) {
                set_ffmpeg_error(err, err_len, "failed to send audio packet", ret);
                failed = 1;
                break;
            }
        } else {
            av_packet_unref(packet);
        }
    }

    if (!failed && !stop_requested(stop_flag)) {
        pulse_error = 0;
        if (pa_simple_drain(pulse, &pulse_error) < 0) {
            set_error(err, err_len, "failed to drain audio: %s", pa_strerror(pulse_error));
            failed = 1;
        }
    }

    av_freep(&out_buffer);
    if (frame != NULL) {
        av_frame_free(&frame);
    }
    if (packet != NULL) {
        av_packet_free(&packet);
    }
    pa_simple_free(pulse);
    swr_free(&swr);
    av_channel_layout_uninit(&src_layout);
    av_channel_layout_uninit(&dst_layout);
    avcodec_free_context(&codec);
    avformat_close_input(&format);

    return failed ? -1 : 0;
}
