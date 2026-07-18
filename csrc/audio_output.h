#ifndef ENZO_AUDIO_OUTPUT_H
#define ENZO_AUDIO_OUTPUT_H

/* Private interface between audio_player.c and audio_output.c. */

#include <pulse/pulseaudio.h>
#include <stddef.h>
#include <stdint.h>

#define ENZO_AUDIO_OUTPUT_RATE 48000
#define ENZO_AUDIO_OUTPUT_CHANNELS 2
#define ENZO_AUDIO_OUTPUT_BYTES_PER_SAMPLE 2

typedef struct EnzoPulseOutput {
    pa_threaded_mainloop *mainloop;
    pa_context *context;
    pa_stream *stream;
    int started;
} EnzoPulseOutput;

typedef struct EnzoAudioClock {
    int generation;
    int initialized;
    int64_t media_origin_micros;
    pa_usec_t pulse_origin_micros;
} EnzoAudioClock;

int enzo_pulse_output_open(
    EnzoPulseOutput *output,
    char *err,
    size_t err_len
);
void enzo_pulse_output_close(EnzoPulseOutput *output);

int enzo_pulse_output_prepare_seek(
    EnzoPulseOutput *output,
    int *corked,
    char *err,
    size_t err_len
);

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
);

void enzo_audio_clock_reset(
    EnzoAudioClock *clock,
    int generation,
    int64_t media_origin_micros,
    int64_t *playback_micros
);

void enzo_audio_clock_update(
    EnzoPulseOutput *output,
    int corked,
    const int *released_seek_generation,
    int seen_seek_generation,
    EnzoAudioClock *clock,
    int64_t *playback_micros
);

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
);

int enzo_pulse_output_drain(
    EnzoPulseOutput *output,
    char *err,
    size_t err_len
);

#endif
