#include "internal.h"

#include <libavutil/error.h>
#include <libavutil/log.h>
#include <libavutil/mathematics.h>
#include <stdarg.h>
#include <stdio.h>

int enzo_stop_requested(const int *stop_flag) {
    return stop_flag != NULL && __atomic_load_n(stop_flag, __ATOMIC_ACQUIRE) != 0;
}

int enzo_pause_requested(const int *pause_flag) {
    return pause_flag != NULL && __atomic_load_n(pause_flag, __ATOMIC_ACQUIRE) != 0;
}

int enzo_mute_requested(const int *mute_flag) {
    return mute_flag != NULL && __atomic_load_n(mute_flag, __ATOMIC_ACQUIRE) != 0;
}

int enzo_seek_generation_value(const int *seek_generation) {
    return seek_generation == NULL
        ? 0
        : __atomic_load_n(seek_generation, __ATOMIC_ACQUIRE);
}

int64_t enzo_seek_micros_value(const int64_t *seek_micros) {
    if (seek_micros == NULL) {
        return 0;
    }
    int64_t value = __atomic_load_n(seek_micros, __ATOMIC_ACQUIRE);
    return value < 0 ? 0 : value;
}

int64_t enzo_stream_timestamp_origin(
    const AVFormatContext *format,
    const AVStream *stream
) {
    if (format->start_time != AV_NOPTS_VALUE) {
        return av_rescale_q(format->start_time, AV_TIME_BASE_Q, stream->time_base);
    }
    return stream->start_time == AV_NOPTS_VALUE ? 0 : stream->start_time;
}

void enzo_atomic_store_generation(int *generation, int value) {
    if (generation != NULL) {
        __atomic_store_n(generation, value, __ATOMIC_RELEASE);
    }
}

void enzo_atomic_store_micros(int64_t *micros, int64_t value) {
    if (micros != NULL) {
        __atomic_store_n(micros, value, __ATOMIC_RELEASE);
    }
}

int enzo_take_seek_request(
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_generation,
    int64_t *micros_out
) {
    int generation = enzo_seek_generation_value(seek_generation);
    if (generation == *seen_generation) {
        return 0;
    }
    *seen_generation = generation;
    *micros_out = enzo_seek_micros_value(seek_micros);
    return 1;
}

void enzo_set_error(char *err, size_t err_len, const char *fmt, ...) {
    if (err == NULL || err_len == 0) {
        return;
    }

    va_list args;
    va_start(args, fmt);
    vsnprintf(err, err_len, fmt, args);
    va_end(args);
}

void enzo_set_ffmpeg_error(char *err, size_t err_len, const char *prefix, int code) {
    char detail[AV_ERROR_MAX_STRING_SIZE] = {0};
    av_strerror(code, detail, sizeof(detail));
    enzo_set_error(err, err_len, "%s: %s", prefix, detail);
}

void enzo_suppress_ffmpeg_logs(void) {
    av_log_set_level(AV_LOG_QUIET);
}

int enzo_open_stream_probe(
    const char *path,
    AVFormatContext **format_out,
    char *err,
    size_t err_len
) {
    AVFormatContext *format = NULL;
    int ret = avformat_open_input(&format, path, NULL, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(
            err,
            err_len,
            "failed to open stream metadata input",
            ret
        );
        return -1;
    }
    ret = avformat_find_stream_info(format, NULL);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to read stream metadata", ret);
        avformat_close_input(&format);
        return -1;
    }
    *format_out = format;
    return 0;
}
