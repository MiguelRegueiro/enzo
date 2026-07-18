#define _POSIX_C_SOURCE 200809L
#define _FILE_OFFSET_BITS 64

#include "internal.h"

#include <errno.h>
#include <libavutil/mem.h>
#include <libavutil/sha.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

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
        enzo_set_error(err, err_len, "failed to seek media fingerprint input: %s", strerror(errno));
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
        if (read != request) {
            if (ferror(file)) {
                enzo_set_error(err, err_len, "failed to read media fingerprint input: %s", strerror(errno));
            } else {
                enzo_set_error(err, err_len, "media changed while computing its fingerprint");
            }
            return -1;
        }
        av_sha_update(sha, buffer, read);
        remaining -= read;
    }
    return 0;
}

int enzo_file_fingerprint(
    const char *path,
    uint64_t len,
    uint64_t chunk_len,
    uint8_t out[32],
    char *err,
    size_t err_len
) {
    if (path == NULL || len == 0 || chunk_len == 0 || chunk_len > len || out == NULL) {
        enzo_set_error(err, err_len, "invalid media fingerprint arguments");
        return -1;
    }

    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        enzo_set_error(err, err_len, "failed to open media for fingerprinting: %s", strerror(errno));
        return -1;
    }
    struct AVSHA *sha = av_sha_alloc();
    if (sha == NULL || av_sha_init(sha, 256) < 0) {
        enzo_set_error(err, err_len, "failed to initialize media fingerprint");
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
