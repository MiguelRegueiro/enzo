# rigoberto

Video player for Kitty-compatible terminals.

Rigoberto renders video frames with the Kitty graphics protocol and plays audio through PulseAudio.
It links directly to FFmpeg libraries for demuxing, decoding, scaling, and resampling.

## Requirements

- Kitty or another terminal that supports the Kitty graphics protocol
- FFmpeg runtime/development libraries: `libavformat`, `libavcodec`, `libavutil`, `libswscale`, `libswresample`
- PulseAudio runtime/development libraries: `libpulse`
- `cc` and `ar` to build the small native media shim

## Run

```sh
cargo run --release -- /path/to/video.mp4
```

Controls:

- Space or right click pauses/resumes playback.
- Left/right arrows seek backward/forward by 5 seconds.
- `q`, Esc, or Ctrl-C quits playback.

The first version intentionally accepts only a video path, plus `--force` for compatible terminals
that do not advertise themselves as Kitty.
