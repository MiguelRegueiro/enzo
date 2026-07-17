#define _POSIX_C_SOURCE 200809L
#define _FILE_OFFSET_BITS 64

#include <errno.h>
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/channel_layout.h>
#include <libavutil/error.h>
#include <libavutil/imgutils.h>
#include <libavutil/log.h>
#include <libavutil/mathematics.h>
#include <libavutil/mem.h>
#include <libavutil/opt.h>
#include <libavutil/sha.h>
#include <libavutil/time.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>
#include <pulse/pulseaudio.h>
#include <math.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define RIG_AUDIO_OUTPUT_RATE 48000
#define RIG_AUDIO_OUTPUT_CHANNELS 2
#define RIG_AUDIO_OUTPUT_BYTES_PER_SAMPLE 2
#define RIG_INFO_TEXT_LEN 64
#define RIG_HDR_NONE 0
#define RIG_HDR_PQ 1
#define RIG_HDR_HLG 2

typedef struct RigVideoInfo {
    uint32_t width;
    uint32_t height;
    double fps;
    double duration;
    int has_audio;
    int seekable;
    char codec[RIG_INFO_TEXT_LEN];
    char profile[RIG_INFO_TEXT_LEN];
    char container[RIG_INFO_TEXT_LEN];
    int hdr;
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
    int has_seek_target;
    double seek_target;
} RigVideoDecoder;

typedef struct RigPulseOutput {
    pa_threaded_mainloop *mainloop;
    pa_context *context;
    pa_stream *stream;
    int started;
} RigPulseOutput;

typedef struct RigAudioConverter {
    SwrContext *swr;
    AVChannelLayout src_layout;
    enum AVSampleFormat src_format;
    int src_rate;
    uint8_t *out_buffer;
    int out_capacity;
    int configured;
} RigAudioConverter;

typedef struct PulseOperationWait {
    pa_threaded_mainloop *mainloop;
    int done;
    int success;
} PulseOperationWait;

void rig_video_decoder_close(RigVideoDecoder *decoder);
static void set_error(char *err, size_t err_len, const char *fmt, ...);

static void store_u64_le(uint8_t out[8], uint64_t value) {
    for (int index = 0; index < 8; index++) {
        out[index] = (uint8_t)(value >> (index * 8));
    }
}

static int hash_file_chunk(
    FILE *file,
    struct AVSHA *sha,
    uint64_t offset,
    uint64_t len,
    char *err,
    size_t err_len
) {
    if (fseeko(file, (off_t)offset, SEEK_SET) != 0) {
        set_error(err, err_len, "failed to seek media fingerprint input: %s", strerror(errno));
        return -1;
    }

    uint8_t encoded_offset[8];
    store_u64_le(encoded_offset, offset);
    av_sha_update(sha, encoded_offset, sizeof(encoded_offset));

    uint8_t buffer[8192];
    uint64_t remaining = len;
    while (remaining > 0) {
        size_t request = remaining < sizeof(buffer) ? (size_t)remaining : sizeof(buffer);
        size_t read = fread(buffer, 1, request, file);
        if (read == 0) {
            if (ferror(file)) {
                set_error(err, err_len, "failed to read media fingerprint input: %s", strerror(errno));
            } else {
                set_error(err, err_len, "media changed while computing its fingerprint");
            }
            return -1;
        }
        av_sha_update(sha, buffer, read);
        remaining -= read;
    }
    return 0;
}

int rig_file_fingerprint(
    const char *path,
    uint64_t len,
    uint64_t chunk_len,
    uint8_t out[32],
    char *err,
    size_t err_len
) {
    if (path == NULL || len == 0 || chunk_len == 0 || chunk_len > len || out == NULL) {
        set_error(err, err_len, "invalid media fingerprint arguments");
        return -1;
    }

    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        set_error(err, err_len, "failed to open media for fingerprinting: %s", strerror(errno));
        return -1;
    }
    struct AVSHA *sha = av_sha_alloc();
    if (sha == NULL || av_sha_init(sha, 256) < 0) {
        set_error(err, err_len, "failed to initialize media fingerprint");
        fclose(file);
        av_free(sha);
        return -1;
    }

    static const uint8_t domain[] = "enzo-sampled-file-v1";
    av_sha_update(sha, domain, sizeof(domain) - 1);
    uint8_t encoded_len[8];
    store_u64_le(encoded_len, len);
    av_sha_update(sha, encoded_len, sizeof(encoded_len));

    uint64_t middle = (len - chunk_len) / 2;
    uint64_t tail = len - chunk_len;
    uint64_t offsets[3] = {0, middle, tail};
    int status = 0;
    for (int index = 0; index < 3; index++) {
        if ((index > 0 && offsets[index] == offsets[index - 1]) ||
            (index == 2 && offsets[index] == offsets[0])) {
            continue;
        }
        if (hash_file_chunk(file, sha, offsets[index], chunk_len, err, err_len) < 0) {
            status = -1;
            break;
        }
    }
    if (status == 0) {
        av_sha_final(sha, out);
    }
    av_free(sha);
    fclose(file);
    return status;
}

static int stop_requested(const int *stop_flag) {
    return stop_flag != NULL && *((volatile const int *)stop_flag) != 0;
}

static int pause_requested(const int *pause_flag) {
    return pause_flag != NULL && *((volatile const int *)pause_flag) != 0;
}

static int mute_requested(const int *mute_flag) {
    return mute_flag != NULL && *((volatile const int *)mute_flag) != 0;
}

static int seek_generation_value(const int *seek_generation) {
    return seek_generation == NULL ? 0 : *((volatile const int *)seek_generation);
}

static int64_t seek_micros_value(const int64_t *seek_micros) {
    if (seek_micros == NULL) {
        return 0;
    }
    int64_t value = *((volatile const int64_t *)seek_micros);
    return value < 0 ? 0 : value;
}

static int take_seek_request(
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_generation,
    int64_t *micros_out
) {
    int generation = seek_generation_value(seek_generation);
    if (generation == *seen_generation) {
        return 0;
    }
    *seen_generation = generation;
    *micros_out = seek_micros_value(seek_micros);
    return 1;
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

static void suppress_ffmpeg_logs(void) {
    av_log_set_level(AV_LOG_QUIET);
}

static void pulse_context_state_callback(pa_context *context, void *userdata) {
    (void)context;
    RigPulseOutput *output = userdata;
    pa_threaded_mainloop_signal(output->mainloop, 0);
}

static void pulse_stream_state_callback(pa_stream *stream, void *userdata) {
    (void)stream;
    RigPulseOutput *output = userdata;
    pa_threaded_mainloop_signal(output->mainloop, 0);
}

static void pulse_stream_success_callback(pa_stream *stream, int success, void *userdata) {
    (void)stream;
    PulseOperationWait *wait = userdata;
    wait->success = success;
    wait->done = 1;
    pa_threaded_mainloop_signal(wait->mainloop, 0);
}

static const char *pulse_output_error(RigPulseOutput *output) {
    if (output->context == NULL) {
        return "unknown PulseAudio error";
    }
    return pa_strerror(pa_context_errno(output->context));
}

static int wait_for_context_ready_locked(
    RigPulseOutput *output,
    char *err,
    size_t err_len
) {
    for (;;) {
        pa_context_state_t state = pa_context_get_state(output->context);
        if (state == PA_CONTEXT_READY) {
            return 0;
        }
        if (!PA_CONTEXT_IS_GOOD(state)) {
            set_error(err, err_len, "failed to connect PulseAudio: %s", pulse_output_error(output));
            return -1;
        }
        pa_threaded_mainloop_wait(output->mainloop);
    }
}

static int wait_for_stream_ready_locked(RigPulseOutput *output, char *err, size_t err_len) {
    for (;;) {
        pa_stream_state_t state = pa_stream_get_state(output->stream);
        if (state == PA_STREAM_READY) {
            return 0;
        }
        if (!PA_STREAM_IS_GOOD(state)) {
            set_error(
                err,
                err_len,
                "failed to create PulseAudio stream: %s",
                pulse_output_error(output)
            );
            return -1;
        }
        pa_threaded_mainloop_wait(output->mainloop);
    }
}

static int wait_for_pulse_operation_locked(
    RigPulseOutput *output,
    pa_operation *operation,
    PulseOperationWait *wait,
    const char *action,
    char *err,
    size_t err_len
) {
    if (operation == NULL) {
        set_error(err, err_len, "%s: %s", action, pulse_output_error(output));
        return -1;
    }

    while (pa_operation_get_state(operation) == PA_OPERATION_RUNNING && !wait->done) {
        pa_threaded_mainloop_wait(output->mainloop);
    }
    pa_operation_unref(operation);

    if (!wait->done || !wait->success) {
        set_error(err, err_len, "%s: %s", action, pulse_output_error(output));
        return -1;
    }
    return 0;
}

static void pulse_output_close(RigPulseOutput *output) {
    if (output == NULL || output->mainloop == NULL) {
        return;
    }

    pa_threaded_mainloop_lock(output->mainloop);
    if (output->stream != NULL) {
        pa_stream_disconnect(output->stream);
        pa_stream_unref(output->stream);
        output->stream = NULL;
    }
    if (output->context != NULL) {
        pa_context_disconnect(output->context);
        pa_context_unref(output->context);
        output->context = NULL;
    }
    pa_threaded_mainloop_unlock(output->mainloop);

    if (output->started) {
        pa_threaded_mainloop_stop(output->mainloop);
        output->started = 0;
    }
    pa_threaded_mainloop_free(output->mainloop);
    output->mainloop = NULL;
}

static int pulse_output_open(RigPulseOutput *output, char *err, size_t err_len) {
    memset(output, 0, sizeof(*output));
    output->mainloop = pa_threaded_mainloop_new();
    if (output->mainloop == NULL) {
        set_error(err, err_len, "failed to allocate PulseAudio mainloop");
        return -1;
    }

    pa_mainloop_api *api = pa_threaded_mainloop_get_api(output->mainloop);
    output->context = pa_context_new(api, "enzo");
    if (output->context == NULL) {
        set_error(err, err_len, "failed to allocate PulseAudio context");
        pulse_output_close(output);
        return -1;
    }
    pa_context_set_state_callback(output->context, pulse_context_state_callback, output);

    if (pa_threaded_mainloop_start(output->mainloop) < 0) {
        set_error(err, err_len, "failed to start PulseAudio mainloop");
        pulse_output_close(output);
        return -1;
    }
    output->started = 1;

    pa_threaded_mainloop_lock(output->mainloop);
    if (pa_context_connect(output->context, NULL, PA_CONTEXT_NOFLAGS, NULL) < 0) {
        set_error(err, err_len, "failed to connect PulseAudio: %s", pulse_output_error(output));
        pa_threaded_mainloop_unlock(output->mainloop);
        pulse_output_close(output);
        return -1;
    }
    if (wait_for_context_ready_locked(output, err, err_len) < 0) {
        pa_threaded_mainloop_unlock(output->mainloop);
        pulse_output_close(output);
        return -1;
    }

    pa_sample_spec sample_spec = {
        .format = PA_SAMPLE_S16LE,
        .rate = RIG_AUDIO_OUTPUT_RATE,
        .channels = RIG_AUDIO_OUTPUT_CHANNELS,
    };
    output->stream = pa_stream_new(output->context, "playback", &sample_spec, NULL);
    if (output->stream == NULL) {
        set_error(err, err_len, "failed to allocate PulseAudio stream: %s", pulse_output_error(output));
        pa_threaded_mainloop_unlock(output->mainloop);
        pulse_output_close(output);
        return -1;
    }
    pa_stream_set_state_callback(output->stream, pulse_stream_state_callback, output);

    pa_buffer_attr buffer_attr = {
        .maxlength = (uint32_t)-1,
        .tlength = RIG_AUDIO_OUTPUT_RATE / 50 * RIG_AUDIO_OUTPUT_CHANNELS *
                   RIG_AUDIO_OUTPUT_BYTES_PER_SAMPLE,
        .prebuf = 0,
        .minreq = RIG_AUDIO_OUTPUT_RATE / 100 * RIG_AUDIO_OUTPUT_CHANNELS *
                  RIG_AUDIO_OUTPUT_BYTES_PER_SAMPLE,
        .fragsize = (uint32_t)-1,
    };
    pa_stream_flags_t flags =
        PA_STREAM_ADJUST_LATENCY | PA_STREAM_INTERPOLATE_TIMING | PA_STREAM_AUTO_TIMING_UPDATE;
    if (pa_stream_connect_playback(output->stream, NULL, &buffer_attr, flags, NULL, NULL) < 0) {
        set_error(err, err_len, "failed to connect PulseAudio stream: %s", pulse_output_error(output));
        pa_threaded_mainloop_unlock(output->mainloop);
        pulse_output_close(output);
        return -1;
    }
    if (wait_for_stream_ready_locked(output, err, err_len) < 0) {
        pa_threaded_mainloop_unlock(output->mainloop);
        pulse_output_close(output);
        return -1;
    }
    pa_threaded_mainloop_unlock(output->mainloop);
    return 0;
}

static int pulse_output_set_corked_locked(
    RigPulseOutput *output,
    int corked,
    char *err,
    size_t err_len
) {
    PulseOperationWait wait = {
        .mainloop = output->mainloop,
        .done = 0,
        .success = 0,
    };
    pa_operation *operation =
        pa_stream_cork(output->stream, corked, pulse_stream_success_callback, &wait);
    return wait_for_pulse_operation_locked(
        output,
        operation,
        &wait,
        corked ? "failed to pause audio" : "failed to resume audio",
        err,
        err_len
    );
}

static int pulse_output_flush_locked(RigPulseOutput *output, char *err, size_t err_len) {
    PulseOperationWait wait = {
        .mainloop = output->mainloop,
        .done = 0,
        .success = 0,
    };
    pa_operation *operation =
        pa_stream_flush(output->stream, pulse_stream_success_callback, &wait);
    return wait_for_pulse_operation_locked(
        output,
        operation,
        &wait,
        "failed to flush audio",
        err,
        err_len
    );
}

static int sync_pulse_pause(
    RigPulseOutput *output,
    const int *stop_flag,
    const int *pause_flag,
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_seek_generation,
    int *corked,
    char *err,
    size_t err_len
) {
    if (pause_requested(pause_flag) && !*corked) {
        pa_threaded_mainloop_lock(output->mainloop);
        int ret = pulse_output_set_corked_locked(output, 1, err, err_len);
        pa_threaded_mainloop_unlock(output->mainloop);
        if (ret < 0) {
            return -1;
        }
        *corked = 1;
    }

    while (pause_requested(pause_flag)) {
        if (stop_requested(stop_flag)) {
            return 1;
        }
        if (seek_generation_value(seek_generation) != *seen_seek_generation) {
            return 2;
        }
        av_usleep(1000);
    }

    if (*corked) {
        pa_threaded_mainloop_lock(output->mainloop);
        int ret = pulse_output_set_corked_locked(output, 0, err, err_len);
        pa_threaded_mainloop_unlock(output->mainloop);
        if (ret < 0) {
            return -1;
        }
        *corked = 0;
    }
    return 0;
}

static int pulse_output_write(
    RigPulseOutput *output,
    const uint8_t *data,
    int bytes,
    const int *stop_flag,
    const int *pause_flag,
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_seek_generation,
    int *corked,
    char *err,
    size_t err_len
) {
    const size_t max_chunk = RIG_AUDIO_OUTPUT_RATE / 100 * RIG_AUDIO_OUTPUT_CHANNELS *
                             RIG_AUDIO_OUTPUT_BYTES_PER_SAMPLE;
    int offset = 0;

    while (offset < bytes) {
        if (stop_requested(stop_flag)) {
            return 0;
        }
        if (seek_generation_value(seek_generation) != *seen_seek_generation) {
            return 1;
        }

        int pause_status = sync_pulse_pause(
            output,
            stop_flag,
            pause_flag,
            seek_generation,
            seek_micros,
            seen_seek_generation,
            corked,
            err,
            err_len
        );
        if (pause_status < 0) {
            return -1;
        }
        if (pause_status > 0 || stop_requested(stop_flag)) {
            return pause_status == 2 ? 1 : 0;
        }

        pa_threaded_mainloop_lock(output->mainloop);
        size_t writable = pa_stream_writable_size(output->stream);
        if (writable == (size_t)-1) {
            set_error(err, err_len, "failed to query PulseAudio stream: %s", pulse_output_error(output));
            pa_threaded_mainloop_unlock(output->mainloop);
            return -1;
        }
        if (writable == 0) {
            pa_threaded_mainloop_unlock(output->mainloop);
            av_usleep(1000);
            continue;
        }

        size_t remaining = (size_t)(bytes - offset);
        size_t chunk = remaining < writable ? remaining : writable;
        if (chunk > max_chunk) {
            chunk = max_chunk;
        }
        if (pa_stream_write(output->stream, data + offset, chunk, NULL, 0, PA_SEEK_RELATIVE)
            < 0) {
            set_error(err, err_len, "failed to write audio: %s", pulse_output_error(output));
            pa_threaded_mainloop_unlock(output->mainloop);
            return -1;
        }
        pa_threaded_mainloop_unlock(output->mainloop);
        offset += (int)chunk;
    }

    return 0;
}

static int pulse_output_drain(RigPulseOutput *output, char *err, size_t err_len) {
    pa_threaded_mainloop_lock(output->mainloop);
    PulseOperationWait wait = {
        .mainloop = output->mainloop,
        .done = 0,
        .success = 0,
    };
    pa_operation *operation =
        pa_stream_drain(output->stream, pulse_stream_success_callback, &wait);
    int ret = wait_for_pulse_operation_locked(
        output,
        operation,
        &wait,
        "failed to drain audio",
        err,
        err_len
    );
    pa_threaded_mainloop_unlock(output->mainloop);
    return ret;
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
    return fps;
}

static void copy_info_text(char *out, const char *text) {
    if (text != NULL) {
        snprintf(out, RIG_INFO_TEXT_LEN, "%s", text);
    }
}

int rig_probe_video(const char *path, RigVideoInfo *out, char *err, size_t err_len) {
    suppress_ffmpeg_logs();

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
        out->hdr = RIG_HDR_PQ;
    } else if (video->codecpar->color_trc == AVCOL_TRC_ARIB_STD_B67) {
        out->hdr = RIG_HDR_HLG;
    } else {
        out->hdr = RIG_HDR_NONE;
    }

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
    suppress_ffmpeg_logs();

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
        int64_t timestamp = decoder->frame->best_effort_timestamp;
        if (timestamp != AV_NOPTS_VALUE) {
            *pts_out = (double)timestamp * av_q2d(decoder->time_base);
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

int rig_video_decoder_seek(
    RigVideoDecoder *decoder,
    double seconds,
    char *err,
    size_t err_len
) {
    if (decoder == NULL) {
        set_error(err, err_len, "invalid video seek arguments");
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
    );
    int ret = av_seek_frame(
        decoder->format,
        decoder->stream_index,
        timestamp,
        AVSEEK_FLAG_BACKWARD
    );
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to seek video", ret);
        return -1;
    }

    avcodec_flush_buffers(decoder->codec);
    decoder->flushing = 0;
    decoder->frame_index = (int64_t)(seconds / decoder->fallback_interval);
    decoder->seek_target = seconds;
    decoder->has_seek_target = 1;
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
    int requested_stream_index,
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

    int stream_index = requested_stream_index;
    if (stream_index >= 0) {
        if ((unsigned int)stream_index >= format->nb_streams ||
            format->streams[stream_index]->codecpar->codec_type != AVMEDIA_TYPE_AUDIO) {
            set_error(err, err_len, "selected audio stream is not available");
            avformat_close_input(&format);
            return -1;
        }
    } else {
        stream_index = av_find_best_stream(format, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0);
    }
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

static void audio_converter_init(RigAudioConverter *converter) {
    memset(converter, 0, sizeof(*converter));
    converter->src_format = AV_SAMPLE_FMT_NONE;
}

static void audio_converter_reset(RigAudioConverter *converter) {
    if (converter == NULL) {
        return;
    }
    swr_free(&converter->swr);
    if (converter->src_layout.nb_channels > 0) {
        av_channel_layout_uninit(&converter->src_layout);
    }
    memset(&converter->src_layout, 0, sizeof(converter->src_layout));
    converter->src_format = AV_SAMPLE_FMT_NONE;
    converter->src_rate = 0;
    converter->configured = 0;
}

static void audio_converter_close(RigAudioConverter *converter) {
    if (converter == NULL) {
        return;
    }
    audio_converter_reset(converter);
    av_freep(&converter->out_buffer);
    converter->out_capacity = 0;
}

static int copy_audio_frame_layout(
    const AVCodecContext *codec,
    const AVFrame *frame,
    AVChannelLayout *layout,
    char *err,
    size_t err_len
) {
    memset(layout, 0, sizeof(*layout));
    if (frame != NULL && frame->ch_layout.nb_channels > 0) {
        int ret = av_channel_layout_copy(layout, &frame->ch_layout);
        if (ret < 0) {
            set_ffmpeg_error(err, err_len, "failed to copy audio frame layout", ret);
            return -1;
        }
        return 0;
    }
    if (codec != NULL && codec->ch_layout.nb_channels > 0) {
        int ret = av_channel_layout_copy(layout, &codec->ch_layout);
        if (ret < 0) {
            set_ffmpeg_error(err, err_len, "failed to copy audio codec layout", ret);
            return -1;
        }
        return 0;
    }

    av_channel_layout_default(layout, RIG_AUDIO_OUTPUT_CHANNELS);
    return 0;
}

static int audio_converter_configure(
    RigAudioConverter *converter,
    const AVCodecContext *codec,
    const AVFrame *frame,
    char *err,
    size_t err_len
) {
    int src_rate = frame->sample_rate > 0 ? frame->sample_rate : codec->sample_rate;
    if (src_rate <= 0) {
        set_error(err, err_len, "invalid audio sample rate");
        return -1;
    }

    enum AVSampleFormat src_format = (enum AVSampleFormat)frame->format;
    if (src_format == AV_SAMPLE_FMT_NONE) {
        src_format = codec->sample_fmt;
    }
    if (src_format == AV_SAMPLE_FMT_NONE) {
        set_error(err, err_len, "invalid audio sample format");
        return -1;
    }

    AVChannelLayout src_layout;
    if (copy_audio_frame_layout(codec, frame, &src_layout, err, err_len) < 0) {
        return -1;
    }

    if (converter->configured && converter->src_rate == src_rate &&
        converter->src_format == src_format &&
        av_channel_layout_compare(&converter->src_layout, &src_layout) == 0) {
        av_channel_layout_uninit(&src_layout);
        return 0;
    }

    AVChannelLayout dst_layout;
    av_channel_layout_default(&dst_layout, RIG_AUDIO_OUTPUT_CHANNELS);

    SwrContext *swr = NULL;
    int ret = swr_alloc_set_opts2(
        &swr,
        &dst_layout,
        AV_SAMPLE_FMT_S16,
        RIG_AUDIO_OUTPUT_RATE,
        &src_layout,
        src_format,
        src_rate,
        0,
        NULL
    );
    av_channel_layout_uninit(&dst_layout);

    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to allocate audio resampler", ret);
        av_channel_layout_uninit(&src_layout);
        swr_free(&swr);
        return -1;
    }
    if (swr == NULL) {
        set_error(err, err_len, "failed to allocate audio resampler");
        av_channel_layout_uninit(&src_layout);
        return -1;
    }

    ret = swr_init(swr);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to initialize audio resampler", ret);
        av_channel_layout_uninit(&src_layout);
        swr_free(&swr);
        return -1;
    }

    audio_converter_reset(converter);
    converter->swr = swr;
    converter->src_layout = src_layout;
    converter->src_format = src_format;
    converter->src_rate = src_rate;
    converter->configured = 1;
    return 0;
}

static int seek_audio_decoder(
    AVFormatContext *format,
    AVCodecContext *codec,
    int stream_index,
    int64_t micros,
    char *err,
    size_t err_len
) {
    AVStream *stream = format->streams[stream_index];
    AVRational micros_base = {1, 1000000};
    int64_t timestamp = av_rescale_q(micros, micros_base, stream->time_base);
    int ret = av_seek_frame(format, stream_index, timestamp, AVSEEK_FLAG_BACKWARD);
    if (ret < 0) {
        set_ffmpeg_error(err, err_len, "failed to seek audio", ret);
        return -1;
    }

    avcodec_flush_buffers(codec);
    return 0;
}

static int sync_audio_seek(
    RigPulseOutput *pulse,
    AVFormatContext *format,
    AVCodecContext *codec,
    RigAudioConverter *converter,
    int stream_index,
    AVPacket *packet,
    AVFrame *frame,
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_seek_generation,
    int *corked,
    int *flushing,
    char *err,
    size_t err_len
) {
    int64_t micros = 0;
    if (!take_seek_request(seek_generation, seek_micros, seen_seek_generation, &micros)) {
        return 0;
    }

    pa_threaded_mainloop_lock(pulse->mainloop);
    if (!*corked) {
        if (pulse_output_set_corked_locked(pulse, 1, err, err_len) < 0) {
            pa_threaded_mainloop_unlock(pulse->mainloop);
            return -1;
        }
        *corked = 1;
    }
    if (pulse_output_flush_locked(pulse, err, err_len) < 0) {
        pa_threaded_mainloop_unlock(pulse->mainloop);
        return -1;
    }
    pa_threaded_mainloop_unlock(pulse->mainloop);

    if (packet != NULL) {
        av_packet_unref(packet);
    }
    if (frame != NULL) {
        av_frame_unref(frame);
    }
    if (seek_audio_decoder(format, codec, stream_index, micros, err, err_len) < 0) {
        return -1;
    }
    audio_converter_reset(converter);
    *flushing = 0;
    return 1;
}

static int write_converted_audio(
    RigAudioConverter *converter,
    AVCodecContext *codec,
    AVFrame *frame,
    RigPulseOutput *pulse,
    const int *stop_flag,
    const int *pause_flag,
    const int *mute_flag,
    const int *seek_generation,
    const int64_t *seek_micros,
    int *seen_seek_generation,
    int *corked,
    char *err,
    size_t err_len
) {
    if (audio_converter_configure(converter, codec, frame, err, err_len) < 0) {
        return -1;
    }

    int out_samples = (int)av_rescale_rnd(
        swr_get_delay(converter->swr, converter->src_rate) + frame->nb_samples,
        RIG_AUDIO_OUTPUT_RATE,
        converter->src_rate,
        AV_ROUND_UP
    );
    if (out_samples <= 0) {
        return 0;
    }

    if (out_samples > converter->out_capacity) {
        av_freep(&converter->out_buffer);
        int line_size = 0;
        int ret = av_samples_alloc(
            &converter->out_buffer,
            &line_size,
            RIG_AUDIO_OUTPUT_CHANNELS,
            out_samples,
            AV_SAMPLE_FMT_S16,
            0
        );
        if (ret < 0) {
            set_ffmpeg_error(err, err_len, "failed to allocate audio buffer", ret);
            return -1;
        }
        converter->out_capacity = out_samples;
    }

    uint8_t *output_planes[1] = {converter->out_buffer};
    int converted = swr_convert(
        converter->swr,
        output_planes,
        out_samples,
        (const uint8_t **)frame->extended_data,
        frame->nb_samples
    );
    if (converted < 0) {
        set_ffmpeg_error(err, err_len, "failed to resample audio", converted);
        return -1;
    }

    int bytes = av_samples_get_buffer_size(
        NULL,
        RIG_AUDIO_OUTPUT_CHANNELS,
        converted,
        AV_SAMPLE_FMT_S16,
        1
    );
    if (bytes < 0) {
        set_ffmpeg_error(err, err_len, "failed to size audio buffer", bytes);
        return -1;
    }

    if (bytes > 0) {
        if (mute_requested(mute_flag)) {
            memset(converter->out_buffer, 0, (size_t)bytes);
        }
        int ret = pulse_output_write(
            pulse,
            converter->out_buffer,
            bytes,
            stop_flag,
            pause_flag,
            seek_generation,
            seek_micros,
            seen_seek_generation,
            corked,
            err,
            err_len
        );
        if (ret < 0) {
            return -1;
        }
        if (ret > 0) {
            return 1;
        }
    }

    return 0;
}

int rig_play_audio(
    const char *path,
    int audio_stream_index,
    const int *stop_flag,
    const int *pause_flag,
    const int *mute_flag,
    const int *seek_generation,
    const int64_t *seek_micros,
    char *err,
    size_t err_len
) {
    suppress_ffmpeg_logs();

    if (path == NULL) {
        set_error(err, err_len, "invalid audio path");
        return -1;
    }

    AVFormatContext *format = NULL;
    AVCodecContext *codec = NULL;
    int stream_index = -1;
    int opened = open_audio_decoder(path, audio_stream_index, &format, &codec, &stream_index, err, err_len);
    if (opened <= 0) {
        return opened;
    }

    RigAudioConverter converter;
    audio_converter_init(&converter);

    RigPulseOutput pulse;
    if (pulse_output_open(&pulse, err, err_len) < 0) {
        avcodec_free_context(&codec);
        avformat_close_input(&format);
        return -1;
    }

    AVPacket *packet = av_packet_alloc();
    AVFrame *frame = av_frame_alloc();
    int ret = 0;
    int failed = 0;
    int flushing = 0;
    int corked = 0;
    int seen_seek_generation = 0;

    if (packet == NULL || frame == NULL) {
        set_error(err, err_len, "failed to allocate audio packet/frame");
        failed = 1;
    }

    while (!failed && !stop_requested(stop_flag)) {
        int seek_status = sync_audio_seek(
            &pulse,
            format,
            codec,
            &converter,
            stream_index,
            packet,
            frame,
            seek_generation,
            seek_micros,
            &seen_seek_generation,
            &corked,
            &flushing,
            err,
            err_len
        );
        if (seek_status < 0) {
            failed = 1;
            break;
        }
        if (seek_status > 0) {
            continue;
        }

        int pause_status = sync_pulse_pause(
            &pulse,
            stop_flag,
            pause_flag,
            seek_generation,
            seek_micros,
            &seen_seek_generation,
            &corked,
            err,
            err_len
        );
        if (pause_status < 0) {
            failed = 1;
            break;
        }
        if (pause_status == 2) {
            continue;
        }
        if (pause_status > 0) {
            break;
        }

        ret = avcodec_receive_frame(codec, frame);
        if (ret == 0) {
            seek_status = sync_audio_seek(
                &pulse,
                format,
                codec,
                &converter,
                stream_index,
                packet,
                frame,
                seek_generation,
                seek_micros,
                &seen_seek_generation,
                &corked,
                &flushing,
                err,
                err_len
            );
            if (seek_status < 0) {
                failed = 1;
                av_frame_unref(frame);
                break;
            }
            if (seek_status > 0) {
                continue;
            }

            pause_status = sync_pulse_pause(
                &pulse,
                stop_flag,
                pause_flag,
                seek_generation,
                seek_micros,
                &seen_seek_generation,
                &corked,
                err,
                err_len
            );
            if (pause_status < 0) {
                failed = 1;
                av_frame_unref(frame);
                break;
            }
            if (pause_status == 2) {
                av_frame_unref(frame);
                continue;
            }
            if (pause_status > 0) {
                av_frame_unref(frame);
                break;
            }
            int write_status = write_converted_audio(
                &converter,
                codec,
                frame,
                &pulse,
                stop_flag,
                pause_flag,
                mute_flag,
                seek_generation,
                seek_micros,
                &seen_seek_generation,
                &corked,
                err,
                err_len
            );
            if (write_status < 0) {
                failed = 1;
            } else if (write_status > 0) {
                av_frame_unref(frame);
                continue;
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
        if (pulse_output_drain(&pulse, err, err_len) < 0) {
            failed = 1;
        }
    }

    audio_converter_close(&converter);
    if (frame != NULL) {
        av_frame_free(&frame);
    }
    if (packet != NULL) {
        av_packet_free(&packet);
    }
    pulse_output_close(&pulse);
    avcodec_free_context(&codec);
    avformat_close_input(&format);

    return failed ? -1 : 0;
}
