#include "internal.h"

#include <libavcodec/avcodec.h>
#include <libavutil/mem.h>
#include <stdint.h>

void enzo_decoded_subtitle_track_free(EnzoDecodedSubtitleTrack *track) {
    if (track == NULL) {
        return;
    }
    for (size_t index = 0; index < track->count; index++) {
        av_free(track->cues[index].text);
    }
    av_free(track->cues);
    track->cues = NULL;
    track->count = 0;
    track->capacity = 0;
}

static int append_decoded_subtitle_cue(
    EnzoDecodedSubtitleTrack *track,
    int64_t start_micros,
    int64_t end_micros,
    int text_kind,
    const char *text,
    char *err,
    size_t err_len
) {
    if (text == NULL || text[0] == '\0' || end_micros <= start_micros) {
        return 0;
    }
    char *text_copy = av_strdup(text);
    if (text_copy == NULL) {
        enzo_set_error(err, err_len, "failed to allocate subtitle cue text");
        return -1;
    }
    if (track->count == track->capacity) {
        size_t capacity = track->capacity == 0 ? 64 : track->capacity * 2;
        if (capacity < track->capacity ||
            capacity > SIZE_MAX / sizeof(*track->cues)) {
            av_free(text_copy);
            enzo_set_error(err, err_len, "subtitle cue count is too large");
            return -1;
        }
        EnzoDecodedSubtitleCue *cues =
            av_realloc_array(track->cues, capacity, sizeof(*track->cues));
        if (cues == NULL) {
            av_free(text_copy);
            enzo_set_error(err, err_len, "failed to allocate subtitle cues");
            return -1;
        }
        track->cues = cues;
        track->capacity = capacity;
    }

    track->cues[track->count] = (EnzoDecodedSubtitleCue) {
        .start_micros = start_micros,
        .end_micros = end_micros,
        .text_kind = text_kind,
        .text = text_copy,
    };
    track->count++;
    return 0;
}

static int append_decoded_subtitle(
    EnzoDecodedSubtitleTrack *track,
    const AVSubtitle *subtitle,
    const AVPacket *packet,
    const AVStream *stream,
    int64_t timestamp_origin,
    char *err,
    size_t err_len
) {
    int64_t origin_micros =
        av_rescale_q(timestamp_origin, stream->time_base, AV_TIME_BASE_Q);
    int64_t base_micros = AV_NOPTS_VALUE;
    if (subtitle->pts != AV_NOPTS_VALUE) {
        base_micros = subtitle->pts - origin_micros;
    } else if (packet->pts != AV_NOPTS_VALUE) {
        base_micros =
            av_rescale_q(packet->pts - timestamp_origin, stream->time_base, AV_TIME_BASE_Q);
    }
    if (base_micros == AV_NOPTS_VALUE) {
        return 0;
    }

    int64_t start_micros =
        base_micros + (int64_t)subtitle->start_display_time * 1000;
    int64_t end_micros = start_micros;
    if (subtitle->end_display_time != UINT32_MAX) {
        end_micros = base_micros + (int64_t)subtitle->end_display_time * 1000;
    }
    if (end_micros <= start_micros && packet->duration > 0) {
        end_micros =
            base_micros + av_rescale_q(packet->duration, stream->time_base, AV_TIME_BASE_Q);
    }
    start_micros = start_micros < 0 ? 0 : start_micros;
    end_micros = end_micros < 0 ? 0 : end_micros;

    for (unsigned int index = 0; index < subtitle->num_rects; index++) {
        const AVSubtitleRect *rect = subtitle->rects[index];
        if (rect == NULL) {
            continue;
        }
        const char *text = NULL;
        int text_kind = ENZO_SUBTITLE_TEXT;
        if (rect->type == SUBTITLE_ASS) {
            text = rect->ass;
            text_kind = ENZO_SUBTITLE_ASS;
        } else if (rect->type == SUBTITLE_TEXT) {
            text = rect->text;
        } else if (rect->text != NULL) {
            text = rect->text;
        } else if (rect->ass != NULL) {
            text = rect->ass;
            text_kind = ENZO_SUBTITLE_ASS;
        }
        if (append_decoded_subtitle_cue(
                track,
                start_micros,
                end_micros,
                text_kind,
                text,
                err,
                err_len
            ) < 0) {
            return -1;
        }
    }
    return 0;
}

static int decode_subtitle_packet(
    AVCodecContext *codec,
    const AVStream *stream,
    int64_t timestamp_origin,
    const AVPacket *packet,
    EnzoDecodedSubtitleTrack *track,
    int *got_subtitle_out,
    char *err,
    size_t err_len
) {
    AVSubtitle subtitle = {0};
    int got_subtitle = 0;
    int ret = avcodec_decode_subtitle2(codec, &subtitle, &got_subtitle, packet);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to decode subtitle packet", ret);
        return -1;
    }
    if (got_subtitle_out != NULL) {
        *got_subtitle_out = got_subtitle;
    }
    int status = 0;
    if (got_subtitle) {
        status = append_decoded_subtitle(
            track,
            &subtitle,
            packet,
            stream,
            timestamp_origin,
            err,
            err_len
        );
        avsubtitle_free(&subtitle);
    }
    return status;
}

int enzo_decode_subtitle_stream(
    const char *path,
    int requested_subtitle_index,
    EnzoDecodedSubtitleTrack *track_out,
    char *err,
    size_t err_len
) {
    enzo_suppress_ffmpeg_logs();
    if (path == NULL || requested_subtitle_index < 0 || track_out == NULL) {
        enzo_set_error(err, err_len, "invalid subtitle decode arguments");
        return -1;
    }
    track_out->cues = NULL;
    track_out->count = 0;
    track_out->capacity = 0;

    AVFormatContext *format = NULL;
    if (enzo_open_stream_probe(path, &format, err, err_len) < 0) {
        return -1;
    }

    int stream_index = -1;
    int subtitle_index = 0;
    for (unsigned int index = 0; index < format->nb_streams; index++) {
        if (format->streams[index]->codecpar->codec_type != AVMEDIA_TYPE_SUBTITLE) {
            continue;
        }
        if (subtitle_index == requested_subtitle_index) {
            stream_index = (int)index;
            break;
        }
        subtitle_index++;
    }
    if (stream_index < 0) {
        enzo_set_error(err, err_len, "selected subtitle stream is not available");
        avformat_close_input(&format);
        return -1;
    }

    AVStream *stream = format->streams[stream_index];
    const AVCodecDescriptor *descriptor =
        avcodec_descriptor_get(stream->codecpar->codec_id);
    if (descriptor == NULL || (descriptor->props & AV_CODEC_PROP_TEXT_SUB) == 0) {
        enzo_set_error(err, err_len, "selected subtitle stream is not text based");
        avformat_close_input(&format);
        return -1;
    }

    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (codec == NULL) {
        enzo_set_error(err, err_len, "failed to find subtitle decoder");
        avformat_close_input(&format);
        return -1;
    }
    AVCodecContext *codec_context = avcodec_alloc_context3(codec);
    if (codec_context == NULL) {
        enzo_set_error(err, err_len, "failed to allocate subtitle decoder");
        avformat_close_input(&format);
        return -1;
    }

    int ret = avcodec_parameters_to_context(codec_context, stream->codecpar);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to copy subtitle codec parameters", ret);
        avcodec_free_context(&codec_context);
        avformat_close_input(&format);
        return -1;
    }
    codec_context->pkt_timebase = stream->time_base;
    ret = avcodec_open2(codec_context, codec, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open subtitle decoder", ret);
        avcodec_free_context(&codec_context);
        avformat_close_input(&format);
        return -1;
    }

    AVPacket *packet = av_packet_alloc();
    if (packet == NULL) {
        enzo_set_error(err, err_len, "failed to allocate subtitle packet");
        avcodec_free_context(&codec_context);
        avformat_close_input(&format);
        return -1;
    }

    int status = 0;
    int64_t timestamp_origin = enzo_stream_timestamp_origin(format, stream);
    while ((ret = av_read_frame(format, packet)) >= 0) {
        if (packet->stream_index == stream_index &&
            decode_subtitle_packet(
                codec_context,
                stream,
                timestamp_origin,
                packet,
                track_out,
                NULL,
                err,
                err_len
            ) < 0) {
            status = -1;
            av_packet_unref(packet);
            break;
        }
        av_packet_unref(packet);
    }
    if (status == 0 && ret != AVERROR_EOF) {
        enzo_set_ffmpeg_error(err, err_len, "failed to read subtitle packets", ret);
        status = -1;
    }

    if (status == 0 && (codec->capabilities & AV_CODEC_CAP_DELAY) != 0) {
        AVPacket flush_packet = {
            .pts = AV_NOPTS_VALUE,
            .dts = AV_NOPTS_VALUE,
        };
        int got_subtitle = 0;
        do {
            if (decode_subtitle_packet(
                    codec_context,
                    stream,
                    timestamp_origin,
                    &flush_packet,
                    track_out,
                    &got_subtitle,
                    err,
                    err_len
                ) < 0) {
                status = -1;
                break;
            }
        } while (got_subtitle);
    }

    av_packet_free(&packet);
    avcodec_free_context(&codec_context);
    avformat_close_input(&format);
    if (status < 0) {
        enzo_decoded_subtitle_track_free(track_out);
    }
    return status;
}
