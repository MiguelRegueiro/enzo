#include "audio_output.h"
#include "internal.h"

#include <libavutil/time.h>
#include <string.h>

typedef struct PulseOperationWait {
    pa_threaded_mainloop *mainloop;
    int done;
    int success;
} PulseOperationWait;

static void pulse_context_state_callback(pa_context *context, void *userdata) {
    (void)context;
    EnzoPulseOutput *output = userdata;
    pa_threaded_mainloop_signal(output->mainloop, 0);
}

static void pulse_stream_state_callback(pa_stream *stream, void *userdata) {
    (void)stream;
    EnzoPulseOutput *output = userdata;
    pa_threaded_mainloop_signal(output->mainloop, 0);
}

static void pulse_stream_success_callback(pa_stream *stream, int success, void *userdata) {
    (void)stream;
    PulseOperationWait *wait = userdata;
    wait->success = success;
    wait->done = 1;
    pa_threaded_mainloop_signal(wait->mainloop, 0);
}

static const char *pulse_output_error(EnzoPulseOutput *output) {
    if (output->context == NULL) {
        return "unknown PulseAudio error";
    }
    return pa_strerror(pa_context_errno(output->context));
}

/*
 * PulseAudio operations normally wake waiters through the threaded mainloop,
 * but Enzo's stop flag lives outside that mainloop. Briefly releasing the lock
 * lets callbacks run and gives shutdown a bounded cancellation point without
 * adding polling to steady-state audio writes.
 */
static int pulse_output_wait_tick_locked(
    EnzoPulseOutput *output,
    const int *stop_flag
) {
    if (enzo_stop_requested(stop_flag)) {
        return 1;
    }
    pa_threaded_mainloop_unlock(output->mainloop);
    av_usleep(1000);
    pa_threaded_mainloop_lock(output->mainloop);
    return enzo_stop_requested(stop_flag);
}

static int wait_for_context_ready_locked(
    EnzoPulseOutput *output,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
    for (;;) {
        pa_context_state_t state = pa_context_get_state(output->context);
        if (state == PA_CONTEXT_READY) {
            return 0;
        }
        if (!PA_CONTEXT_IS_GOOD(state)) {
            enzo_set_error(err, err_len, "failed to connect PulseAudio: %s", pulse_output_error(output));
            return -1;
        }
        if (pulse_output_wait_tick_locked(output, stop_flag)) {
            return 1;
        }
    }
}

static int wait_for_stream_ready_locked(
    EnzoPulseOutput *output,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
    for (;;) {
        pa_stream_state_t state = pa_stream_get_state(output->stream);
        if (state == PA_STREAM_READY) {
            return 0;
        }
        if (!PA_STREAM_IS_GOOD(state)) {
            enzo_set_error(
                err,
                err_len,
                "failed to create PulseAudio stream: %s",
                pulse_output_error(output)
            );
            return -1;
        }
        if (pulse_output_wait_tick_locked(output, stop_flag)) {
            return 1;
        }
    }
}

static int wait_for_pulse_operation_locked(
    EnzoPulseOutput *output,
    pa_operation *operation,
    PulseOperationWait *wait,
    const int *stop_flag,
    const char *action,
    char *err,
    size_t err_len
) {
    if (operation == NULL) {
        enzo_set_error(err, err_len, "%s: %s", action, pulse_output_error(output));
        return -1;
    }

    while (pa_operation_get_state(operation) == PA_OPERATION_RUNNING && !wait->done) {
        if (pulse_output_wait_tick_locked(output, stop_flag)) {
            pa_operation_cancel(operation);
            pa_operation_unref(operation);
            return 1;
        }
    }
    pa_operation_unref(operation);

    if (!wait->done || !wait->success) {
        enzo_set_error(err, err_len, "%s: %s", action, pulse_output_error(output));
        return -1;
    }
    return 0;
}

void enzo_pulse_output_close(EnzoPulseOutput *output) {
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

int enzo_pulse_output_open(
    EnzoPulseOutput *output,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
    memset(output, 0, sizeof(*output));
    output->mainloop = pa_threaded_mainloop_new();
    if (output->mainloop == NULL) {
        enzo_set_error(err, err_len, "failed to allocate PulseAudio mainloop");
        return -1;
    }

    pa_mainloop_api *api = pa_threaded_mainloop_get_api(output->mainloop);
    output->context = pa_context_new(api, "enzo");
    if (output->context == NULL) {
        enzo_set_error(err, err_len, "failed to allocate PulseAudio context");
        enzo_pulse_output_close(output);
        return -1;
    }
    pa_context_set_state_callback(output->context, pulse_context_state_callback, output);

    if (pa_threaded_mainloop_start(output->mainloop) < 0) {
        enzo_set_error(err, err_len, "failed to start PulseAudio mainloop");
        enzo_pulse_output_close(output);
        return -1;
    }
    output->started = 1;

    pa_threaded_mainloop_lock(output->mainloop);
    if (pa_context_connect(output->context, NULL, PA_CONTEXT_NOFLAGS, NULL) < 0) {
        enzo_set_error(err, err_len, "failed to connect PulseAudio: %s", pulse_output_error(output));
        pa_threaded_mainloop_unlock(output->mainloop);
        enzo_pulse_output_close(output);
        return -1;
    }
    int ready_status =
        wait_for_context_ready_locked(output, stop_flag, err, err_len);
    if (ready_status != 0) {
        pa_threaded_mainloop_unlock(output->mainloop);
        enzo_pulse_output_close(output);
        return ready_status;
    }

    pa_sample_spec sample_spec = {
        .format = PA_SAMPLE_S16LE,
        .rate = ENZO_AUDIO_OUTPUT_RATE,
        .channels = ENZO_AUDIO_OUTPUT_CHANNELS,
    };
    output->stream = pa_stream_new(output->context, "playback", &sample_spec, NULL);
    if (output->stream == NULL) {
        enzo_set_error(err, err_len, "failed to allocate PulseAudio stream: %s", pulse_output_error(output));
        pa_threaded_mainloop_unlock(output->mainloop);
        enzo_pulse_output_close(output);
        return -1;
    }
    pa_stream_set_state_callback(output->stream, pulse_stream_state_callback, output);

    pa_buffer_attr buffer_attr = {
        .maxlength = (uint32_t)-1,
        .tlength = ENZO_AUDIO_OUTPUT_RATE / 50 * ENZO_AUDIO_OUTPUT_CHANNELS *
                   ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE,
        .prebuf = 0,
        .minreq = ENZO_AUDIO_OUTPUT_RATE / 100 * ENZO_AUDIO_OUTPUT_CHANNELS *
                  ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE,
        .fragsize = (uint32_t)-1,
    };
    pa_stream_flags_t flags =
        PA_STREAM_ADJUST_LATENCY | PA_STREAM_INTERPOLATE_TIMING |
        PA_STREAM_AUTO_TIMING_UPDATE | PA_STREAM_START_CORKED;
    if (pa_stream_connect_playback(output->stream, NULL, &buffer_attr, flags, NULL, NULL) < 0) {
        enzo_set_error(err, err_len, "failed to connect PulseAudio stream: %s", pulse_output_error(output));
        pa_threaded_mainloop_unlock(output->mainloop);
        enzo_pulse_output_close(output);
        return -1;
    }
    ready_status = wait_for_stream_ready_locked(
        output,
        stop_flag,
        err,
        err_len
    );
    if (ready_status != 0) {
        pa_threaded_mainloop_unlock(output->mainloop);
        enzo_pulse_output_close(output);
        return ready_status;
    }
    pa_threaded_mainloop_unlock(output->mainloop);
    return 0;
}

static int pulse_output_set_corked_locked(
    EnzoPulseOutput *output,
    const int *stop_flag,
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
        stop_flag,
        corked ? "failed to pause audio" : "failed to resume audio",
        err,
        err_len
    );
}

static int pulse_output_flush_locked(
    EnzoPulseOutput *output,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
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
        stop_flag,
        "failed to flush audio",
        err,
        err_len
    );
}

int enzo_pulse_output_prepare_seek(
    EnzoPulseOutput *output,
    const int *stop_flag,
    int *corked,
    char *err,
    size_t err_len
) {
    pa_threaded_mainloop_lock(output->mainloop);
    if (!*corked) {
        int cork_status = pulse_output_set_corked_locked(
            output,
            stop_flag,
            1,
            err,
            err_len
        );
        if (cork_status != 0) {
            pa_threaded_mainloop_unlock(output->mainloop);
            return cork_status;
        }
    }
    *corked = 1;
    int status = pulse_output_flush_locked(
        output,
        stop_flag,
        err,
        err_len
    );
    pa_threaded_mainloop_unlock(output->mainloop);
    return status;
}

static int pulse_output_update_timing_locked(
    EnzoPulseOutput *output,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
    PulseOperationWait wait = {
        .mainloop = output->mainloop,
        .done = 0,
        .success = 0,
    };
    pa_operation *operation = pa_stream_update_timing_info(
        output->stream,
        pulse_stream_success_callback,
        &wait
    );
    return wait_for_pulse_operation_locked(
        output,
        operation,
        &wait,
        stop_flag,
        "failed to update audio timing",
        err,
        err_len
    );
}

int enzo_sync_pulse_pause(
    EnzoPulseOutput *output,
    const int *stop_flag,
    const int *pause_flag,
    const int *seek_generation,
    const int *released_seek_generation,
    int *seen_seek_generation,
    int *corked,
    char *err,
    size_t err_len
) {
    int seek_held =
        enzo_seek_generation_value(released_seek_generation) != *seen_seek_generation;
    if ((enzo_pause_requested(pause_flag) || seek_held) && !*corked) {
        pa_threaded_mainloop_lock(output->mainloop);
        int ret = pulse_output_set_corked_locked(
            output,
            stop_flag,
            1,
            err,
            err_len
        );
        pa_threaded_mainloop_unlock(output->mainloop);
        if (ret != 0) {
            return ret;
        }
        *corked = 1;
    }

    while (enzo_pause_requested(pause_flag)) {
        if (enzo_stop_requested(stop_flag)) {
            return 1;
        }
        if (enzo_seek_generation_value(seek_generation) != *seen_seek_generation) {
            return 2;
        }
        av_usleep(1000);
    }

    seek_held =
        enzo_seek_generation_value(released_seek_generation) != *seen_seek_generation;
    if (*corked && !seek_held) {
        pa_threaded_mainloop_lock(output->mainloop);
        int ret = pulse_output_update_timing_locked(
            output,
            stop_flag,
            err,
            err_len
        );
        if (ret == 0) {
            ret = pulse_output_set_corked_locked(
                output,
                stop_flag,
                0,
                err,
                err_len
            );
        }
        pa_threaded_mainloop_unlock(output->mainloop);
        if (ret != 0) {
            return ret;
        }
        *corked = 0;
    }
    return 0;
}

void enzo_audio_clock_reset(
    EnzoAudioClock *clock,
    int generation,
    int64_t media_origin_micros,
    int64_t *playback_micros
) {
    clock->generation = generation;
    clock->initialized = 0;
    clock->media_origin_micros = media_origin_micros;
    clock->pulse_origin_micros = 0;
    enzo_atomic_store_micros(playback_micros, media_origin_micros);
}

void enzo_audio_clock_update(
    EnzoPulseOutput *output,
    int corked,
    const int *released_seek_generation,
    int seen_seek_generation,
    EnzoAudioClock *clock,
    int64_t *playback_micros
) {
    if (corked ||
        enzo_seek_generation_value(released_seek_generation) != seen_seek_generation ||
        clock->generation != seen_seek_generation) {
        return;
    }

    pa_usec_t pulse_micros = 0;
    pa_threaded_mainloop_lock(output->mainloop);
    int status = pa_stream_get_time(output->stream, &pulse_micros);
    pa_threaded_mainloop_unlock(output->mainloop);
    if (status < 0) {
        return;
    }
    if (!clock->initialized) {
        clock->pulse_origin_micros = pulse_micros;
        clock->initialized = 1;
    }

    int64_t elapsed = pulse_micros >= clock->pulse_origin_micros
        ? (int64_t)(pulse_micros - clock->pulse_origin_micros)
        : 0;
    enzo_atomic_store_micros(
        playback_micros,
        clock->media_origin_micros + elapsed
    );
}

int enzo_pulse_output_write(
    EnzoPulseOutput *output,
    const uint8_t *data,
    int bytes,
    const int *stop_flag,
    const int *pause_flag,
    const int *seek_generation,
    const int *released_seek_generation,
    int *buffered_seek_generation,
    int *seen_seek_generation,
    int *corked,
    EnzoAudioClock *clock,
    int64_t *playback_micros,
    char *err,
    size_t err_len
) {
    const size_t max_chunk = ENZO_AUDIO_OUTPUT_RATE / 100 * ENZO_AUDIO_OUTPUT_CHANNELS *
                             ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE;
    int offset = 0;

    while (offset < bytes) {
        if (enzo_stop_requested(stop_flag)) {
            return 0;
        }
        if (enzo_seek_generation_value(seek_generation) != *seen_seek_generation) {
            return 1;
        }

        int pause_status = enzo_sync_pulse_pause(
            output,
            stop_flag,
            pause_flag,
            seek_generation,
            released_seek_generation,
            seen_seek_generation,
            corked,
            err,
            err_len
        );
        if (pause_status < 0) {
            return -1;
        }
        if (pause_status > 0 || enzo_stop_requested(stop_flag)) {
            return pause_status == 2 ? 1 : 0;
        }
        enzo_audio_clock_update(
            output,
            *corked,
            released_seek_generation,
            *seen_seek_generation,
            clock,
            playback_micros
        );

        pa_threaded_mainloop_lock(output->mainloop);
        size_t writable = pa_stream_writable_size(output->stream);
        if (writable == (size_t)-1) {
            enzo_set_error(err, err_len, "failed to query PulseAudio stream: %s", pulse_output_error(output));
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
            enzo_set_error(err, err_len, "failed to write audio: %s", pulse_output_error(output));
            pa_threaded_mainloop_unlock(output->mainloop);
            return -1;
        }
        pa_threaded_mainloop_unlock(output->mainloop);
        offset += (int)chunk;
        enzo_atomic_store_generation(buffered_seek_generation, *seen_seek_generation);
        enzo_audio_clock_update(
            output,
            *corked,
            released_seek_generation,
            *seen_seek_generation,
            clock,
            playback_micros
        );
    }

    return 0;
}

int enzo_pulse_output_drain(
    EnzoPulseOutput *output,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
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
        stop_flag,
        "failed to drain audio",
        err,
        err_len
    );
    pa_threaded_mainloop_unlock(output->mainloop);
    return ret;
}
