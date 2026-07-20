#include "internal.h"

#include <libavcodec/avcodec.h>
#include <libavutil/mem.h>
#include <stdint.h>
#include <string.h>

void enzo_decoded_subtitle_track_free(EnzoDecodedSubtitleTrack *track) {
    if (track == NULL) {
        return;
    }
    for (size_t index = 0; index < track->count; index++) {
        av_free(track->cues[index].text);
        av_free(track->cues[index].bitmap_indices);
    }
    av_free(track->cues);
    track->cues = NULL;
    track->count = 0;
    track->capacity = 0;
    track->canvas_width = 0;
    track->canvas_height = 0;
}

static int ensure_decoded_subtitle_capacity(
    EnzoDecodedSubtitleTrack *track,
    char *err,
    size_t err_len
) {
    if (track->count < track->capacity) {
        return 0;
    }
    size_t capacity = track->capacity == 0 ? 64 : track->capacity * 2;
    if (capacity < track->capacity ||
        capacity > SIZE_MAX / sizeof(*track->cues)) {
        enzo_set_error(err, err_len, "subtitle cue count is too large");
        return -1;
    }
    EnzoDecodedSubtitleCue *cues =
        av_realloc_array(track->cues, capacity, sizeof(*track->cues));
    if (cues == NULL) {
        enzo_set_error(err, err_len, "failed to allocate subtitle cues");
        return -1;
    }
    track->cues = cues;
    track->capacity = capacity;
    return 0;
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
    if (ensure_decoded_subtitle_capacity(track, err, err_len) < 0) {
        av_free(text_copy);
        return -1;
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

static int append_decoded_bitmap_cue(
    EnzoDecodedSubtitleTrack *track,
    int64_t start_micros,
    int64_t end_micros,
    const AVSubtitleRect *rect,
    char *err,
    size_t err_len
) {
    if (rect == NULL || rect->x < 0 || rect->y < 0 ||
        rect->w <= 0 || rect->h <= 0 ||
        rect->data[0] == NULL || rect->data[1] == NULL ||
        rect->linesize[0] == 0) {
        return 0;
    }
    size_t width = (size_t)rect->w;
    size_t height = (size_t)rect->h;
    if (height > SIZE_MAX / width) {
        enzo_set_error(err, err_len, "subtitle bitmap is too large");
        return -1;
    }
    size_t bitmap_len = width * height;
    size_t pitch = (size_t)(rect->linesize[0] < 0
        ? -(int64_t)rect->linesize[0]
        : rect->linesize[0]);
    if (pitch < width) {
        enzo_set_error(err, err_len, "subtitle bitmap pitch is invalid");
        return -1;
    }
    uint8_t *indices = av_malloc(bitmap_len);
    if (indices == NULL) {
        enzo_set_error(err, err_len, "failed to allocate subtitle bitmap");
        return -1;
    }
    for (size_t row = 0; row < height; row++) {
        size_t source_row =
            rect->linesize[0] > 0 ? row : height - 1 - row;
        memcpy(indices + row * width, rect->data[0] + source_row * pitch, width);
    }

    if (ensure_decoded_subtitle_capacity(track, err, err_len) < 0) {
        av_free(indices);
        return -1;
    }

    EnzoDecodedSubtitleCue cue = {
        .start_micros = start_micros,
        .end_micros = end_micros,
        .text_kind = ENZO_SUBTITLE_BITMAP,
        .bitmap_x = (uint32_t)rect->x,
        .bitmap_y = (uint32_t)rect->y,
        .bitmap_width = (uint32_t)rect->w,
        .bitmap_height = (uint32_t)rect->h,
        .bitmap_indices = indices,
    };
    size_t palette_bytes = rect->nb_colors > 0
        ? (size_t)rect->nb_colors * 4
        : 0;
    if (palette_bytes > ENZO_SUBTITLE_PALETTE_BYTES) {
        palette_bytes = ENZO_SUBTITLE_PALETTE_BYTES;
    }
    memcpy(cue.palette_rgba, rect->data[1], palette_bytes);
    track->cues[track->count] = cue;
    track->count++;
    return 0;
}

static void close_open_bitmap_cues(
    EnzoDecodedSubtitleTrack *track,
    size_t *open_bitmap_start,
    int64_t end_micros
) {
    if (*open_bitmap_start == SIZE_MAX) {
        return;
    }
    for (size_t index = *open_bitmap_start; index < track->count; index++) {
        EnzoDecodedSubtitleCue *cue = &track->cues[index];
        if (cue->text_kind == ENZO_SUBTITLE_BITMAP &&
            end_micros > cue->start_micros) {
            cue->end_micros = end_micros;
        }
    }
    *open_bitmap_start = SIZE_MAX;
}

static void update_pgs_bitmap_canvas(
    EnzoDecodedSubtitleTrack *track,
    const AVPacket *packet
) {
    if (packet->data == NULL || packet->size <= 0) {
        return;
    }
    size_t packet_size = (size_t)packet->size;
    size_t offset = 0;
    while (offset < packet_size) {
        if (packet_size - offset >= 10 &&
            packet->data[offset] == 'P' && packet->data[offset + 1] == 'G') {
            offset += 10;
        }
        if ((size_t)packet->size - offset < 3) {
            return;
        }
        uint8_t segment_type = packet->data[offset];
        size_t segment_len =
            ((size_t)packet->data[offset + 1] << 8) |
            packet->data[offset + 2];
        offset += 3;
        if (segment_len > packet_size - offset) {
            return;
        }
        if (segment_type == 0x16 && segment_len >= 4) {
            uint32_t width =
                ((uint32_t)packet->data[offset] << 8) |
                packet->data[offset + 1];
            uint32_t height =
                ((uint32_t)packet->data[offset + 2] << 8) |
                packet->data[offset + 3];
            if (width > 0 && height > 0) {
                track->canvas_width = width;
                track->canvas_height = height;
            }
        }
        offset += segment_len;
    }
}

static int append_decoded_subtitle(
    EnzoDecodedSubtitleTrack *track,
    const AVSubtitle *subtitle,
    const AVPacket *packet,
    const AVStream *stream,
    int64_t timestamp_origin,
    size_t *open_bitmap_start,
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

    int has_bitmap = 0;
    for (unsigned int index = 0; index < subtitle->num_rects; index++) {
        const AVSubtitleRect *rect = subtitle->rects[index];
        if (rect != NULL && rect->type == SUBTITLE_BITMAP) {
            has_bitmap = 1;
            break;
        }
    }
    if (subtitle->num_rects == 0 || has_bitmap) {
        close_open_bitmap_cues(track, open_bitmap_start, start_micros);
    }
    size_t bitmap_start = track->count;

    for (unsigned int index = 0; index < subtitle->num_rects; index++) {
        const AVSubtitleRect *rect = subtitle->rects[index];
        if (rect == NULL) {
            continue;
        }
        if (rect->type == SUBTITLE_BITMAP) {
            int64_t bitmap_end =
                end_micros > start_micros ? end_micros : INT64_MAX;
            if (append_decoded_bitmap_cue(
                    track,
                    start_micros,
                    bitmap_end,
                    rect,
                    err,
                    err_len
                ) < 0) {
                return -1;
            }
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
    if (has_bitmap && track->count > bitmap_start) {
        *open_bitmap_start = bitmap_start;
    }
    return 0;
}

static int decode_subtitle_packet(
    AVCodecContext *codec,
    const AVStream *stream,
    int64_t timestamp_origin,
    const AVPacket *packet,
    EnzoDecodedSubtitleTrack *track,
    size_t *open_bitmap_start,
    int *got_subtitle_out,
    char *err,
    size_t err_len
) {
    const AVCodecDescriptor *descriptor = avcodec_descriptor_get(codec->codec_id);
    if (descriptor != NULL && (descriptor->props & AV_CODEC_PROP_BITMAP_SUB) != 0) {
        update_pgs_bitmap_canvas(track, packet);
    }
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
            open_bitmap_start,
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
    track_out->canvas_width = 0;
    track_out->canvas_height = 0;

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
    if (descriptor == NULL ||
        (descriptor->props & (AV_CODEC_PROP_TEXT_SUB | AV_CODEC_PROP_BITMAP_SUB)) == 0) {
        enzo_set_error(err, err_len, "selected subtitle stream is not supported");
        avformat_close_input(&format);
        return -1;
    }
    for (unsigned int index = 0; index < format->nb_streams; index++) {
        const AVCodecParameters *parameters = format->streams[index]->codecpar;
        if (parameters->codec_type == AVMEDIA_TYPE_VIDEO &&
            parameters->width > 0 && parameters->height > 0) {
            track_out->canvas_width = (uint32_t)parameters->width;
            track_out->canvas_height = (uint32_t)parameters->height;
            break;
        }
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
    size_t open_bitmap_start = SIZE_MAX;
    int64_t timestamp_origin = enzo_stream_timestamp_origin(format, stream);
    while ((ret = av_read_frame(format, packet)) >= 0) {
        if (packet->stream_index == stream_index &&
            decode_subtitle_packet(
                codec_context,
                stream,
                timestamp_origin,
                packet,
                track_out,
                &open_bitmap_start,
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
                    &open_bitmap_start,
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
