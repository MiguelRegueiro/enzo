#ifndef ENZO_MEDIA_INPUT_H
#define ENZO_MEDIA_INPUT_H

/* FFmpeg input ownership and I/O policy. Private to the C media backend. */

#include <libavformat/avformat.h>
#include <stddef.h>
#include <stdint.h>

typedef struct EnzoInput EnzoInput;

int enzo_input_open(
    const char *path,
    const int *stop_flag,
    EnzoInput **input_out
);

int enzo_input_open_probe(
    const char *path,
    const int *stop_flag,
    EnzoInput **input_out,
    char *err,
    size_t err_len
);

AVFormatContext *enzo_input_format(EnzoInput *input);

int enzo_input_find_stream_info(
    EnzoInput *input,
    const int *stop_flag
);

int enzo_input_read_frame(
    EnzoInput *input,
    AVPacket *packet,
    const int *stop_flag
);

int enzo_input_seek_frame(
    EnzoInput *input,
    int stream_index,
    int64_t timestamp,
    int flags,
    const int *stop_flag
);

void enzo_input_close(EnzoInput **input);

#endif
