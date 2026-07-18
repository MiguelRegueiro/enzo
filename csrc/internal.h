#ifndef ENZO_MEDIA_INTERNAL_H
#define ENZO_MEDIA_INTERNAL_H

/* Shared implementation helpers. This is not part of the Rust-facing ABI. */

#include "media.h"

#include <libavformat/avformat.h>
#include <stdint.h>

int enzo_stop_requested(const int *stop_flag);
int enzo_pause_requested(const int *pause_flag);
int enzo_mute_requested(const int *mute_flag);
int enzo_seek_generation_value(const int *seek_generation);
int64_t enzo_seek_micros_value(const int64_t *seek_micros);
int64_t enzo_stream_timestamp_origin(
    const AVFormatContext *format,
    const AVStream *stream
);
void enzo_atomic_store_generation(int *generation, int value);
void enzo_atomic_store_micros(int64_t *micros, int64_t value);
int enzo_take_seek_request(
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_generation,
    int64_t *micros_out
);

void enzo_set_error(char *err, size_t err_len, const char *fmt, ...);
void enzo_set_ffmpeg_error(
    char *err,
    size_t err_len,
    const char *prefix,
    int code
);
void enzo_suppress_ffmpeg_logs(void);

int enzo_open_stream_probe(
    const char *path,
    AVFormatContext **format_out,
    char *err,
    size_t err_len
);

#endif
