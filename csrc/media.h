#ifndef ENZO_MEDIA_H
#define ENZO_MEDIA_H

/*
 * The complete C ABI consumed by Rust.
 *
 * Keep implementation details out and keep declarations synchronized with
 * src/media/ffi.rs.
 */

#include <stddef.h>
#include <stdint.h>

#define ENZO_INFO_TEXT_LEN 64
#define ENZO_TRACK_TEXT_LEN 128

#define ENZO_HDR_NONE 0
#define ENZO_HDR_PQ 1
#define ENZO_HDR_HLG 2

#define ENZO_SUBTITLE_TEXT 1
#define ENZO_SUBTITLE_ASS 2
#define ENZO_SUBTITLE_BITMAP 3
#define ENZO_SUBTITLE_PALETTE_BYTES (256 * 4)

typedef struct EnzoVideoInfo {
    uint32_t width;
    uint32_t height;
    double fps;
    double duration;
    int has_audio;
    int seekable;
    char codec[ENZO_INFO_TEXT_LEN];
    char profile[ENZO_INFO_TEXT_LEN];
    char container[ENZO_INFO_TEXT_LEN];
    int hdr;
} EnzoVideoInfo;

typedef struct EnzoAudioTrackInfo {
    int stream_index;
    int channels;
    int sample_rate;
    int is_default;
    char codec[ENZO_TRACK_TEXT_LEN];
    char channel_layout[ENZO_TRACK_TEXT_LEN];
    char language[ENZO_TRACK_TEXT_LEN];
    char title[ENZO_TRACK_TEXT_LEN];
} EnzoAudioTrackInfo;

typedef struct EnzoSubtitleStreamInfo {
    int subtitle_index;
    int is_default;
    int is_forced;
    char codec[ENZO_TRACK_TEXT_LEN];
    char language[ENZO_TRACK_TEXT_LEN];
    char title[ENZO_TRACK_TEXT_LEN];
} EnzoSubtitleStreamInfo;

typedef struct EnzoDecodedSubtitleCue {
    int64_t start_micros;
    int64_t end_micros;
    int text_kind;
    char *text;
    uint32_t bitmap_x;
    uint32_t bitmap_y;
    uint32_t bitmap_width;
    uint32_t bitmap_height;
    uint8_t *bitmap_indices;
    uint8_t palette_rgba[ENZO_SUBTITLE_PALETTE_BYTES];
} EnzoDecodedSubtitleCue;

typedef struct EnzoDecodedSubtitleTrack {
    EnzoDecodedSubtitleCue *cues;
    size_t count;
    size_t capacity;
    uint32_t canvas_width;
    uint32_t canvas_height;
} EnzoDecodedSubtitleTrack;

typedef struct EnzoVideoDecoder EnzoVideoDecoder;

int enzo_file_fingerprint(
    const char *path,
    uint64_t len,
    uint64_t chunk_len,
    uint8_t out[32],
    char *err,
    size_t err_len
);

int enzo_probe_video(
    const char *path,
    EnzoVideoInfo *out,
    char *err,
    size_t err_len
);

int enzo_probe_audio_tracks(
    const char *path,
    EnzoAudioTrackInfo **tracks_out,
    size_t *count_out,
    char *err,
    size_t err_len
);

void enzo_audio_tracks_free(EnzoAudioTrackInfo *tracks);

int enzo_probe_subtitle_streams(
    const char *path,
    EnzoSubtitleStreamInfo **streams_out,
    size_t *count_out,
    char *err,
    size_t err_len
);

void enzo_subtitle_streams_free(EnzoSubtitleStreamInfo *streams);

int enzo_decode_subtitle_stream(
    const char *path,
    int requested_subtitle_index,
    EnzoDecodedSubtitleTrack *track_out,
    char *err,
    size_t err_len
);

void enzo_decoded_subtitle_track_free(EnzoDecodedSubtitleTrack *track);

int enzo_video_decoder_open(
    const char *path,
    int out_width,
    int out_height,
    double fps,
    EnzoVideoDecoder **out,
    char *err,
    size_t err_len
);

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
);

int enzo_video_decoder_seek(
    EnzoVideoDecoder *decoder,
    double seconds,
    int exact,
    char *err,
    size_t err_len
);

void enzo_video_decoder_close(EnzoVideoDecoder *decoder);

int enzo_audio_seek_trim_samples(
    int64_t frame_timestamp,
    int64_t timestamp_origin,
    int time_base_num,
    int time_base_den,
    int frame_samples,
    int source_rate,
    int64_t target_micros,
    int delayed_output_samples,
    int converted_samples
);

int enzo_audio_seek_leading_silence_samples(
    int64_t frame_timestamp,
    int64_t timestamp_origin,
    int time_base_num,
    int time_base_den,
    int64_t target_micros
);

int enzo_play_audio(
    const char *path,
    int audio_stream_index,
    const int *stop_flag,
    const int *pause_flag,
    const int *mute_flag,
    const int *seek_generation,
    const int64_t *seek_micros,
    const int *released_seek_generation,
    int *applied_seek_generation,
    int *buffered_seek_generation,
    int64_t *playback_micros,
    char *err,
    size_t err_len
);

#endif
