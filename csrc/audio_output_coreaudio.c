/*
 * CoreAudio implementation of the private audio output interface used by
 * audio_player.c. It mirrors the PulseAudio backend in audio_output.c:
 *
 * - "corked" maps to the output unit being stopped (streams open corked);
 * - the writable size is the free space in a lock-free SPSC ring buffer that
 *   the real-time render callback drains;
 * - flush discards the ring while the unit is stopped;
 * - stream time counts frames that were actually rendered from the ring
 *   (underrun silence is not counted, matching Pulse with prebuf = 0).
 */
#ifdef __APPLE__

#include "audio_output.h"
#include "internal.h"

#include <AudioToolbox/AudioToolbox.h>
#include <libavutil/time.h>
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

#define ENZO_COREAUDIO_FRAME_BYTES \
    (ENZO_AUDIO_OUTPUT_CHANNELS * ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE)
/* ~85 ms of 48 kHz stereo S16; must be a power of two and a frame multiple. */
#define ENZO_COREAUDIO_RING_BYTES 16384u

struct EnzoCoreAudioOutput {
    AudioComponentInstance unit;
    int initialized;
    uint8_t ring[ENZO_COREAUDIO_RING_BYTES];
    _Atomic uint64_t read_pos;
    _Atomic uint64_t write_pos;
    _Atomic uint64_t played_frames;
    _Atomic int running;
};

static size_t ring_used(const EnzoCoreAudioOutput *impl) {
    uint64_t write = atomic_load_explicit(&impl->write_pos, memory_order_acquire);
    uint64_t read = atomic_load_explicit(&impl->read_pos, memory_order_acquire);
    return (size_t)(write - read);
}

static OSStatus render_callback(
    void *userdata,
    AudioUnitRenderActionFlags *flags,
    const AudioTimeStamp *timestamp,
    UInt32 bus,
    UInt32 frame_count,
    AudioBufferList *buffers
) {
    (void)timestamp;
    (void)bus;
    EnzoCoreAudioOutput *impl = userdata;
    uint8_t *out = buffers->mBuffers[0].mData;
    size_t wanted = (size_t)frame_count * ENZO_COREAUDIO_FRAME_BYTES;
    if (wanted > buffers->mBuffers[0].mDataByteSize) {
        wanted = buffers->mBuffers[0].mDataByteSize;
    }

    uint64_t read = atomic_load_explicit(&impl->read_pos, memory_order_relaxed);
    uint64_t write = atomic_load_explicit(&impl->write_pos, memory_order_acquire);
    size_t available = (size_t)(write - read);
    size_t copy = available < wanted ? available : wanted;
    copy -= copy % ENZO_COREAUDIO_FRAME_BYTES;

    size_t start = (size_t)(read & (ENZO_COREAUDIO_RING_BYTES - 1));
    size_t first = ENZO_COREAUDIO_RING_BYTES - start;
    if (first > copy) {
        first = copy;
    }
    memcpy(out, impl->ring + start, first);
    memcpy(out + first, impl->ring, copy - first);
    if (copy < wanted) {
        memset(out + copy, 0, wanted - copy);
    }
    if (copy == 0 && flags != NULL) {
        *flags |= kAudioUnitRenderAction_OutputIsSilence;
    }

    atomic_store_explicit(&impl->read_pos, read + copy, memory_order_release);
    atomic_fetch_add_explicit(
        &impl->played_frames,
        copy / ENZO_COREAUDIO_FRAME_BYTES,
        memory_order_release
    );
    return noErr;
}

void enzo_pulse_output_close(EnzoPulseOutput *output) {
    if (output == NULL || output->impl == NULL) {
        return;
    }
    EnzoCoreAudioOutput *impl = output->impl;
    if (impl->unit != NULL) {
        if (atomic_load(&impl->running)) {
            AudioOutputUnitStop(impl->unit);
            atomic_store(&impl->running, 0);
        }
        if (impl->initialized) {
            AudioUnitUninitialize(impl->unit);
        }
        AudioComponentInstanceDispose(impl->unit);
    }
    free(impl);
    output->impl = NULL;
}

int enzo_pulse_output_open(
    EnzoPulseOutput *output,
    const int *stop_flag,
    char *err,
    size_t err_len
) {
    memset(output, 0, sizeof(*output));
    if (enzo_stop_requested(stop_flag)) {
        return 1;
    }

    EnzoCoreAudioOutput *impl = calloc(1, sizeof(*impl));
    if (impl == NULL) {
        enzo_set_error(err, err_len, "failed to allocate CoreAudio output");
        return -1;
    }
    output->impl = impl;

    AudioComponentDescription description = {
        .componentType = kAudioUnitType_Output,
        .componentSubType = kAudioUnitSubType_DefaultOutput,
        .componentManufacturer = kAudioUnitManufacturer_Apple,
    };
    AudioComponent component = AudioComponentFindNext(NULL, &description);
    if (component == NULL) {
        enzo_set_error(err, err_len, "failed to find the default CoreAudio output");
        enzo_pulse_output_close(output);
        return -1;
    }
    OSStatus status = AudioComponentInstanceNew(component, &impl->unit);
    if (status != noErr) {
        enzo_set_error(err, err_len, "failed to open CoreAudio output (OSStatus %d)", (int)status);
        enzo_pulse_output_close(output);
        return -1;
    }

    AudioStreamBasicDescription format = {
        .mSampleRate = ENZO_AUDIO_OUTPUT_RATE,
        .mFormatID = kAudioFormatLinearPCM,
        .mFormatFlags = kAudioFormatFlagIsSignedInteger | kAudioFormatFlagIsPacked,
        .mBytesPerPacket = ENZO_COREAUDIO_FRAME_BYTES,
        .mFramesPerPacket = 1,
        .mBytesPerFrame = ENZO_COREAUDIO_FRAME_BYTES,
        .mChannelsPerFrame = ENZO_AUDIO_OUTPUT_CHANNELS,
        .mBitsPerChannel = ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE * 8,
    };
    status = AudioUnitSetProperty(
        impl->unit,
        kAudioUnitProperty_StreamFormat,
        kAudioUnitScope_Input,
        0,
        &format,
        sizeof(format)
    );
    if (status != noErr) {
        enzo_set_error(err, err_len, "failed to set CoreAudio stream format (OSStatus %d)", (int)status);
        enzo_pulse_output_close(output);
        return -1;
    }

    AURenderCallbackStruct callback = {
        .inputProc = render_callback,
        .inputProcRefCon = impl,
    };
    status = AudioUnitSetProperty(
        impl->unit,
        kAudioUnitProperty_SetRenderCallback,
        kAudioUnitScope_Input,
        0,
        &callback,
        sizeof(callback)
    );
    if (status != noErr) {
        enzo_set_error(err, err_len, "failed to set CoreAudio render callback (OSStatus %d)", (int)status);
        enzo_pulse_output_close(output);
        return -1;
    }

    status = AudioUnitInitialize(impl->unit);
    if (status != noErr) {
        enzo_set_error(err, err_len, "failed to initialize CoreAudio output (OSStatus %d)", (int)status);
        enzo_pulse_output_close(output);
        return -1;
    }
    impl->initialized = 1;
    /* Like PA_STREAM_START_CORKED: the unit is started on the first uncork. */
    return enzo_stop_requested(stop_flag) ? 1 : 0;
}

static int coreaudio_set_corked(
    EnzoCoreAudioOutput *impl,
    int corked,
    char *err,
    size_t err_len
) {
    int running = atomic_load(&impl->running);
    if (corked == !running) {
        return 0;
    }
    OSStatus status = corked ? AudioOutputUnitStop(impl->unit) : AudioOutputUnitStart(impl->unit);
    if (status != noErr) {
        enzo_set_error(
            err,
            err_len,
            "%s (OSStatus %d)",
            corked ? "failed to pause audio" : "failed to resume audio",
            (int)status
        );
        return -1;
    }
    atomic_store(&impl->running, !corked);
    return 0;
}

int enzo_pulse_output_prepare_seek(
    EnzoPulseOutput *output,
    const int *stop_flag,
    int *corked,
    char *err,
    size_t err_len
) {
    EnzoCoreAudioOutput *impl = output->impl;
    if (enzo_stop_requested(stop_flag)) {
        return 1;
    }
    if (coreaudio_set_corked(impl, 1, err, err_len) != 0) {
        return -1;
    }
    *corked = 1;
    /* The render callback is not running once the unit is stopped. */
    atomic_store_explicit(
        &impl->read_pos,
        atomic_load_explicit(&impl->write_pos, memory_order_acquire),
        memory_order_release
    );
    return 0;
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
    EnzoCoreAudioOutput *impl = output->impl;
    int seek_held =
        enzo_seek_generation_value(released_seek_generation) != *seen_seek_generation;
    if ((enzo_pause_requested(pause_flag) || seek_held) && !*corked) {
        if (coreaudio_set_corked(impl, 1, err, err_len) != 0) {
            return -1;
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
        if (coreaudio_set_corked(impl, 0, err, err_len) != 0) {
            return -1;
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

static uint64_t coreaudio_stream_micros(EnzoCoreAudioOutput *impl) {
    uint64_t frames = atomic_load_explicit(&impl->played_frames, memory_order_acquire);
    return frames * 1000000u / ENZO_AUDIO_OUTPUT_RATE;
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

    uint64_t stream_micros = coreaudio_stream_micros(output->impl);
    if (!clock->initialized) {
        clock->pulse_origin_micros = stream_micros;
        clock->initialized = 1;
    }

    int64_t elapsed = stream_micros >= clock->pulse_origin_micros
        ? (int64_t)(stream_micros - clock->pulse_origin_micros)
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
    EnzoCoreAudioOutput *impl = output->impl;
    const size_t max_chunk = ENZO_AUDIO_OUTPUT_RATE / 100 * ENZO_COREAUDIO_FRAME_BYTES;
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

        size_t writable = ENZO_COREAUDIO_RING_BYTES - ring_used(impl);
        writable -= writable % ENZO_COREAUDIO_FRAME_BYTES;
        if (writable == 0) {
            av_usleep(1000);
            continue;
        }

        size_t remaining = (size_t)(bytes - offset);
        size_t chunk = remaining < writable ? remaining : writable;
        if (chunk > max_chunk) {
            chunk = max_chunk;
        }

        uint64_t write = atomic_load_explicit(&impl->write_pos, memory_order_relaxed);
        size_t start = (size_t)(write & (ENZO_COREAUDIO_RING_BYTES - 1));
        size_t first = ENZO_COREAUDIO_RING_BYTES - start;
        if (first > chunk) {
            first = chunk;
        }
        memcpy(impl->ring + start, data + offset, first);
        memcpy(impl->ring, data + offset + first, chunk - first);
        atomic_store_explicit(&impl->write_pos, write + chunk, memory_order_release);

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
    (void)err;
    (void)err_len;
    EnzoCoreAudioOutput *impl = output->impl;
    while (ring_used(impl) >= ENZO_COREAUDIO_FRAME_BYTES) {
        if (enzo_stop_requested(stop_flag)) {
            return 1;
        }
        if (!atomic_load(&impl->running)) {
            /* A stopped unit never drains; nothing more will be played. */
            return 0;
        }
        av_usleep(1000);
    }
    return 0;
}

#endif
