# C media backend

This directory implements Enzo's C interoperability layer for FFmpeg,
libswscale, libswresample, PulseAudio, HarfBuzz, and FriBidi.

Rust calls only the functions declared in `media.h` and `text_layout.h`.
Everything in `internal.h` and `audio_output.h` is private to this directory.

## ABI contract

`media.h` and `text_layout.h` are the canonical C interfaces.
`src/media/ffi.rs` and `src/text_layout.rs` are their exact Rust mirrors, so
every change to an ABI function, type, constant, or ownership rule must update
both sides in the same commit.

Rust code outside the `media` module must use safe wrappers instead of calling
the raw FFI declarations directly. Before committing an ABI change, run the
Rust tests, Clippy with warnings denied, the strict C compiler checks, and the
sanitizer command below.

## Modules

- `common.c` — error reporting, cross-thread control values, timestamps, and
  shared FFmpeg input setup.
- `fingerprint.c` — sampled SHA-256 fingerprints used by resume records.
- `probe.c` — video, audio-track, and subtitle-stream metadata.
- `subtitle_decoder.c` — embedded text/bitmap subtitle decoding and cue ownership.
- `text_layout.c` — bidirectional run resolution, cluster-safe font fallback,
  and positioned glyph shaping.
- `video_decoder.c` — video decode, seeking, and RGB24 scaling.
- `audio_output.c` — PulseAudio connection, buffering, pause state, and clock.
- `audio_player.c` — audio decode, seeking, resampling, and playback
  orchestration.

## Ownership rules

- Objects returned through an output pointer are owned by Rust after a
  successful call.
- Every returned allocation has a matching `enzo_*_free` or `enzo_*_close`
  function in `media.h`.
- `EnzoVideoDecoder` is opaque outside `video_decoder.c`.
- `EnzoPulseOutput` and `EnzoAudioClock` are internal to the audio modules.
- Error buffers are borrowed for the duration of a call and contain a
  NUL-terminated message when an operation fails.

`build.rs` lists every translation unit explicitly and compiles them into the
single static `enzo_media` library linked into the Rust executable.

## Validation

The normal Rust tests exercise the C backend through its public ABI. For local
memory-safety validation, run the same tests with Clang's AddressSanitizer and
UndefinedBehaviorSanitizer in an isolated target directory:

```sh
CARGO_TARGET_DIR=target/sanitized \
CC=clang \
CFLAGS="-fsanitize=address,undefined -fno-omit-frame-pointer" \
RUSTFLAGS="-C linker=clang -C link-arg=-fsanitize=address -C link-arg=-fsanitize=undefined" \
ASAN_OPTIONS="detect_leaks=0" \
cargo test --all-targets
```

Leak detection is disabled because it is incompatible with ptrace-based
sandboxes; AddressSanitizer's bounds and lifetime checks remain enabled.
