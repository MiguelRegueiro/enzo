# rigoberto

Video player for Kitty-compatible terminals.

Rigoberto renders video frames with the Kitty graphics protocol and plays audio through PulseAudio.
It links directly to FFmpeg libraries for demuxing, decoding, scaling, and resampling.

## Requirements

- Kitty or another terminal that supports the Kitty graphics protocol
- FFmpeg runtime/development libraries: `libavformat`, `libavcodec`, `libavutil`, `libswscale`, `libswresample`
- PulseAudio runtime/development libraries: `libpulse`
- FreeType runtime/development libraries: `libfreetype`
- `cc` and `ar` to build the small native media shim

## Run

```sh
cargo run --release -- /path/to/video.mp4
```

Run without a path to open the drop target:

```sh
cargo run --release
```

Controls:

- Drop a file or URL on the launcher to play it.
- Space or right click pauses/resumes playback.
- Left/right arrows seek backward/forward by 5 seconds.
- Click or drag the progress bar to seek.
- `q`, Esc, or Ctrl-C quits playback.

The playback overlay appears while paused, after seeking, and on mouse activity.

Use `--force` for compatible terminals that do not advertise themselves as Kitty.
