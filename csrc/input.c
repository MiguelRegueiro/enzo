#include "internal.h"

#include <ctype.h>
#include <errno.h>
#include <libavutil/dict.h>
#include <libavutil/error.h>
#include <libavutil/mem.h>
#include <libavutil/opt.h>
#include <libavutil/time.h>
#include <string.h>

#define ENZO_IO_TIMEOUT_US (60LL * AV_TIME_BASE)
#define ENZO_NETWORK_PROTOCOLS "http,https,tcp,tls,httpproxy,data,crypto"
#define ENZO_LOCAL_PROTOCOLS "file," ENZO_NETWORK_PROTOCOLS

enum EnzoInputOrigin {
    ENZO_INPUT_LOCAL,
    ENZO_INPUT_NETWORK,
};

enum EnzoInterruptReason {
    ENZO_INTERRUPT_NONE,
    ENZO_INTERRUPT_STOP,
    ENZO_INTERRUPT_TIMEOUT,
};

struct EnzoInput {
    AVFormatContext *format;
    enum EnzoInputOrigin origin;
    const int *stop_flag;
    int64_t deadline_us;
    int interrupt_reason;
};

static const char *enzo_protocol_whitelist(const EnzoInput *input) {
    /*
     * tcp, tls, httpproxy, and crypto are transport layers used internally by
     * HTTP(S). enzo_nested_url_allowed() separately prevents them from being
     * opened as user-visible URLs.
     */
    if (input->origin == ENZO_INPUT_NETWORK) {
        return ENZO_NETWORK_PROTOCOLS;
    }
    return ENZO_LOCAL_PROTOCOLS;
}

static size_t enzo_url_scheme_len(const char *url) {
    if (url == NULL || !isalpha((unsigned char)url[0])) {
        return 0;
    }

    for (size_t index = 1; url[index] != '\0'; index++) {
        unsigned char ch = (unsigned char)url[index];
        if (ch == ':') {
            return index;
        }
        if (!isalnum(ch) && ch != '+' && ch != '-' && ch != '.') {
            return 0;
        }
    }
    return 0;
}

static int enzo_scheme_is(const char *url, size_t scheme_len, const char *scheme) {
    return strlen(scheme) == scheme_len &&
        strncmp(url, scheme, scheme_len) == 0;
}

static int enzo_classify_input(
    const char *path,
    enum EnzoInputOrigin *origin_out
) {
    size_t scheme_len = enzo_url_scheme_len(path);
    if (scheme_len == 0 || enzo_scheme_is(path, scheme_len, "file")) {
        *origin_out = ENZO_INPUT_LOCAL;
        return 0;
    }
    if (enzo_scheme_is(path, scheme_len, "http") ||
        enzo_scheme_is(path, scheme_len, "https")) {
        *origin_out = ENZO_INPUT_NETWORK;
        return 0;
    }
    return AVERROR(EACCES);
}

static int enzo_nested_url_allowed(const EnzoInput *input, const char *url) {
    size_t scheme_len = enzo_url_scheme_len(url);
    if (scheme_len == 0) {
        return input->origin == ENZO_INPUT_LOCAL;
    }

    /* Cross-origin HTTP(S) is required by ordinary multi-CDN HLS playlists. */
    if (enzo_scheme_is(url, scheme_len, "http") ||
        enzo_scheme_is(url, scheme_len, "https") ||
        enzo_scheme_is(url, scheme_len, "data") ||
        enzo_scheme_is(url, scheme_len, "crypto+http") ||
        enzo_scheme_is(url, scheme_len, "crypto+https")) {
        return 1;
    }

    if (input->origin == ENZO_INPUT_LOCAL &&
        (enzo_scheme_is(url, scheme_len, "file") ||
         enzo_scheme_is(url, scheme_len, "crypto") ||
         enzo_scheme_is(url, scheme_len, "crypto+file"))) {
        return 1;
    }
    return 0;
}

static int enzo_interrupt_callback(void *opaque) {
    EnzoInput *input = opaque;
    const int *stop_flag =
        __atomic_load_n(&input->stop_flag, __ATOMIC_ACQUIRE);
    if (enzo_stop_requested(stop_flag)) {
        __atomic_store_n(
            &input->interrupt_reason,
            ENZO_INTERRUPT_STOP,
            __ATOMIC_RELEASE
        );
        return 1;
    }

    int64_t deadline_us =
        __atomic_load_n(&input->deadline_us, __ATOMIC_ACQUIRE);
    if (deadline_us > 0 && av_gettime_relative() >= deadline_us) {
        __atomic_store_n(
            &input->interrupt_reason,
            ENZO_INTERRUPT_TIMEOUT,
            __ATOMIC_RELEASE
        );
        return 1;
    }
    return 0;
}

static int enzo_io_open(
    AVFormatContext *format,
    AVIOContext **io_out,
    const char *url,
    int flags,
    AVDictionary **options
) {
    EnzoInput *input = format == NULL ? NULL : format->opaque;
    if (input == NULL || io_out == NULL || url == NULL) {
        return AVERROR(EINVAL);
    }
    if ((flags & AVIO_FLAG_WRITE) != 0 ||
        !enzo_nested_url_allowed(input, url)) {
        return AVERROR(EACCES);
    }

    AVDictionary *local_options = NULL;
    AVDictionary **open_options = options == NULL ? &local_options : options;
    int ret = av_dict_set(
        open_options,
        "protocol_whitelist",
        enzo_protocol_whitelist(input),
        0
    );
    if (ret >= 0) {
        ret = av_dict_set_int(
            open_options,
            "rw_timeout",
            ENZO_IO_TIMEOUT_US,
            0
        );
    }
    if (ret >= 0) {
        ret = av_dict_set_int(
            open_options,
            "timeout",
            ENZO_IO_TIMEOUT_US,
            0
        );
    }
    if (ret >= 0) {
        const AVIOInterruptCB interrupt = {
            .callback = enzo_interrupt_callback,
            .opaque = input,
        };
        ret = avio_open2(io_out, url, flags, &interrupt, open_options);
    }
    av_dict_free(&local_options);
    return ret;
}

static void enzo_input_begin_io(EnzoInput *input, const int *stop_flag) {
    __atomic_store_n(
        &input->interrupt_reason,
        ENZO_INTERRUPT_NONE,
        __ATOMIC_RELEASE
    );
    __atomic_store_n(&input->stop_flag, stop_flag, __ATOMIC_RELEASE);
    __atomic_store_n(
        &input->deadline_us,
        av_gettime_relative() + ENZO_IO_TIMEOUT_US,
        __ATOMIC_RELEASE
    );
}

static int enzo_input_finish_io(EnzoInput *input, int ret) {
    int reason = __atomic_load_n(
        &input->interrupt_reason,
        __ATOMIC_ACQUIRE
    );
    __atomic_store_n(&input->deadline_us, 0, __ATOMIC_RELEASE);
    __atomic_store_n(&input->stop_flag, NULL, __ATOMIC_RELEASE);
    if (ret == AVERROR_EXIT && reason == ENZO_INTERRUPT_TIMEOUT) {
        return AVERROR(ETIMEDOUT);
    }
    return ret;
}

int enzo_input_open(
    const char *path,
    const int *stop_flag,
    EnzoInput **input_out
) {
    if (path == NULL || input_out == NULL) {
        return AVERROR(EINVAL);
    }
    *input_out = NULL;

    EnzoInput *input = av_mallocz(sizeof(*input));
    if (input == NULL) {
        return AVERROR(ENOMEM);
    }
    int ret = enzo_classify_input(path, &input->origin);
    if (ret < 0) {
        av_free(input);
        return ret;
    }

    input->format = avformat_alloc_context();
    if (input->format == NULL) {
        av_free(input);
        return AVERROR(ENOMEM);
    }
    input->format->opaque = input;
    input->format->io_open = enzo_io_open;
    input->format->interrupt_callback = (AVIOInterruptCB){
        .callback = enzo_interrupt_callback,
        .opaque = input,
    };

    ret = av_opt_set(
        input->format,
        "protocol_whitelist",
        enzo_protocol_whitelist(input),
        0
    );
    AVDictionary *options = NULL;
    if (ret >= 0) {
        // Allow HLS media segments served with nonstandard filename extensions.
        ret = av_dict_set(&options, "extension_picky", "0", 0);
    }
    if (ret >= 0) {
        enzo_input_begin_io(input, stop_flag);
        ret = avformat_open_input(&input->format, path, NULL, &options);
        ret = enzo_input_finish_io(input, ret);
    }
    av_dict_free(&options);
    if (ret < 0) {
        enzo_input_close(&input);
        return ret;
    }

    *input_out = input;
    return 0;
}

int enzo_input_open_probe(
    const char *path,
    const int *stop_flag,
    EnzoInput **input_out,
    char *err,
    size_t err_len
) {
    EnzoInput *input = NULL;
    int ret = enzo_input_open(path, stop_flag, &input);
    if (ret < 0) {
        enzo_set_ffmpeg_error(
            err,
            err_len,
            "failed to open stream metadata input",
            ret
        );
        return -1;
    }
    ret = enzo_input_find_stream_info(input, stop_flag);
    if (ret < 0) {
        enzo_set_ffmpeg_error(err, err_len, "failed to read stream metadata", ret);
        enzo_input_close(&input);
        return -1;
    }
    *input_out = input;
    return 0;
}

AVFormatContext *enzo_input_format(EnzoInput *input) {
    return input == NULL ? NULL : input->format;
}

int enzo_input_find_stream_info(
    EnzoInput *input,
    const int *stop_flag
) {
    if (input == NULL || input->format == NULL) {
        return AVERROR(EINVAL);
    }
    enzo_input_begin_io(input, stop_flag);
    int ret = avformat_find_stream_info(input->format, NULL);
    return enzo_input_finish_io(input, ret);
}

int enzo_input_read_frame(
    EnzoInput *input,
    AVPacket *packet,
    const int *stop_flag
) {
    if (input == NULL || input->format == NULL || packet == NULL) {
        return AVERROR(EINVAL);
    }
    enzo_input_begin_io(input, stop_flag);
    int ret = av_read_frame(input->format, packet);
    return enzo_input_finish_io(input, ret);
}

int enzo_input_seek_frame(
    EnzoInput *input,
    int stream_index,
    int64_t timestamp,
    int flags,
    const int *stop_flag
) {
    if (input == NULL || input->format == NULL) {
        return AVERROR(EINVAL);
    }
    enzo_input_begin_io(input, stop_flag);
    int ret = av_seek_frame(
        input->format,
        stream_index,
        timestamp,
        flags
    );
    return enzo_input_finish_io(input, ret);
}

void enzo_input_close(EnzoInput **input) {
    if (input == NULL || *input == NULL) {
        return;
    }
    if ((*input)->format != NULL && (*input)->format->iformat == NULL) {
        avformat_free_context((*input)->format);
        (*input)->format = NULL;
    } else {
        avformat_close_input(&(*input)->format);
    }
    av_freep(input);
}
