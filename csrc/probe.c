#include "internal.h"

#include <libavcodec/avcodec.h>
#include <libavutil/channel_layout.h>
#include <libavutil/mem.h>
#include <stdio.h>

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
    return fps;
}

static void copy_info_text(char *out, const char *text) {
    if (text != NULL) {
        snprintf(out, ENZO_INFO_TEXT_LEN, "%s", text);
    }
}

int enzo_probe_video(const char *path, EnzoVideoInfo *out, char *err, size_t err_len) {
    enzo_suppress_ffmpeg_logs();

    if (path == NULL || out == NULL) {
        enzo_set_error(err, err_len, "invalid probe arguments");
        return -1;
    }

    AVFormatContext *format = NULL;
    int ret = avformat_open_input(&format, path, NULL, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to open input", ret);
        return -1;
    }

    ret = avformat_find_stream_info(format, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to read stream info", ret);
        avformat_close_input(&format);
        return -1;
    }

    int video_index = av_find_best_stream(format, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    if (video_index < 0) {
        enzo_set_error(err, err_len, "input has no video stream");
        avformat_close_input(&format);
        return -1;
    }

    AVStream *video = format->streams[video_index];
    out->width = (uint32_t)video->codecpar->width;
    out->height = (uint32_t)video->codecpar->height;
    out->fps = stream_fps(video);
    out->duration = format->duration > 0 ? (double)format->duration / (double)AV_TIME_BASE : 0.0;
    out->has_audio =
        av_find_best_stream(format, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0) >= 0;
    out->seekable =
        format->duration > 0 &&
        format->pb != NULL &&
        (format->pb->seekable & AVIO_SEEKABLE_NORMAL) != 0;
    copy_info_text(out->codec, avcodec_get_name(video->codecpar->codec_id));
    copy_info_text(
        out->profile,
        avcodec_profile_name(video->codecpar->codec_id, video->codecpar->profile)
    );
    copy_info_text(out->container, format->iformat->name);
    if (video->codecpar->color_trc == AVCOL_TRC_SMPTE2084) {
        out->hdr = ENZO_HDR_PQ;
    } else if (video->codecpar->color_trc == AVCOL_TRC_ARIB_STD_B67) {
        out->hdr = ENZO_HDR_HLG;
    } else {
        out->hdr = ENZO_HDR_NONE;
    }

    avformat_close_input(&format);
    return 0;
}

static const char *stream_metadata(const AVStream *stream, const char *key) {
    const AVDictionaryEntry *entry = av_dict_get(stream->metadata, key, NULL, 0);
    return entry == NULL ? NULL : entry->value;
}

static void copy_track_text(char out[ENZO_TRACK_TEXT_LEN], const char *text) {
    if (text != NULL) {
        snprintf(out, ENZO_TRACK_TEXT_LEN, "%s", text);
    }
}

void enzo_audio_tracks_free(EnzoAudioTrackInfo *tracks) {
    av_free(tracks);
}

int enzo_probe_audio_tracks(
    const char *path,
    EnzoAudioTrackInfo **tracks_out,
    size_t *count_out,
    char *err,
    size_t err_len
) {
    enzo_suppress_ffmpeg_logs();
    if (path == NULL || tracks_out == NULL || count_out == NULL) {
        enzo_set_error(err, err_len, "invalid audio track probe arguments");
        return -1;
    }
    *tracks_out = NULL;
    *count_out = 0;

    AVFormatContext *format = NULL;
    if (enzo_open_stream_probe(path, &format, err, err_len) < 0) {
        return -1;
    }

    size_t count = 0;
    for (unsigned int index = 0; index < format->nb_streams; index++) {
        if (format->streams[index]->codecpar->codec_type == AVMEDIA_TYPE_AUDIO) {
            count++;
        }
    }
    if (count == 0) {
        avformat_close_input(&format);
        return 0;
    }

    EnzoAudioTrackInfo *tracks = av_calloc(count, sizeof(*tracks));
    if (tracks == NULL) {
        enzo_set_error(err, err_len, "failed to allocate audio track metadata");
        avformat_close_input(&format);
        return -1;
    }

    size_t track_index = 0;
    for (unsigned int index = 0; index < format->nb_streams; index++) {
        const AVStream *stream = format->streams[index];
        const AVCodecParameters *parameters = stream->codecpar;
        if (parameters->codec_type != AVMEDIA_TYPE_AUDIO) {
            continue;
        }

        EnzoAudioTrackInfo *track = &tracks[track_index++];
        track->stream_index = (int)index;
        track->channels = parameters->ch_layout.nb_channels;
        track->sample_rate = parameters->sample_rate;
        track->is_default = (stream->disposition & AV_DISPOSITION_DEFAULT) != 0;
        copy_track_text(track->codec, avcodec_get_name(parameters->codec_id));
        if (parameters->ch_layout.nb_channels > 0) {
            av_channel_layout_describe(
                &parameters->ch_layout,
                track->channel_layout,
                sizeof(track->channel_layout)
            );
        }
        copy_track_text(track->language, stream_metadata(stream, "language"));
        copy_track_text(track->title, stream_metadata(stream, "title"));
    }

    avformat_close_input(&format);
    *tracks_out = tracks;
    *count_out = count;
    return 0;
}

void enzo_subtitle_streams_free(EnzoSubtitleStreamInfo *streams) {
    av_free(streams);
}

int enzo_probe_subtitle_streams(
    const char *path,
    EnzoSubtitleStreamInfo **streams_out,
    size_t *count_out,
    char *err,
    size_t err_len
) {
    enzo_suppress_ffmpeg_logs();
    if (path == NULL || streams_out == NULL || count_out == NULL) {
        enzo_set_error(err, err_len, "invalid subtitle stream probe arguments");
        return -1;
    }
    *streams_out = NULL;
    *count_out = 0;

    AVFormatContext *format = NULL;
    if (enzo_open_stream_probe(path, &format, err, err_len) < 0) {
        return -1;
    }

    size_t count = 0;
    for (unsigned int index = 0; index < format->nb_streams; index++) {
        if (format->streams[index]->codecpar->codec_type == AVMEDIA_TYPE_SUBTITLE) {
            count++;
        }
    }
    if (count == 0) {
        avformat_close_input(&format);
        return 0;
    }

    EnzoSubtitleStreamInfo *streams = av_calloc(count, sizeof(*streams));
    if (streams == NULL) {
        enzo_set_error(err, err_len, "failed to allocate subtitle stream metadata");
        avformat_close_input(&format);
        return -1;
    }

    size_t subtitle_index = 0;
    for (unsigned int index = 0; index < format->nb_streams; index++) {
        const AVStream *stream = format->streams[index];
        const AVCodecParameters *parameters = stream->codecpar;
        if (parameters->codec_type != AVMEDIA_TYPE_SUBTITLE) {
            continue;
        }

        EnzoSubtitleStreamInfo *subtitle = &streams[subtitle_index];
        subtitle->subtitle_index = (int)subtitle_index;
        subtitle->is_default = (stream->disposition & AV_DISPOSITION_DEFAULT) != 0;
        subtitle->is_forced = (stream->disposition & AV_DISPOSITION_FORCED) != 0;
        copy_track_text(subtitle->codec, avcodec_get_name(parameters->codec_id));
        copy_track_text(subtitle->language, stream_metadata(stream, "language"));
        copy_track_text(subtitle->title, stream_metadata(stream, "title"));
        subtitle_index++;
    }

    avformat_close_input(&format);
    *streams_out = streams;
    *count_out = count;
    return 0;
}
