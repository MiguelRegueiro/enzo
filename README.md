# enzo

Video player for Kitty-compatible terminals.

Enzo renders video frames with the Kitty graphics protocol and plays audio through PulseAudio.
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

Sidecar subtitles are loaded automatically from `/path/to/video.srt`.
Use `--sub-file` to select a specific SRT file:

```sh
cargo run --release -- --sub-file /path/to/subtitles.srt /path/to/video.mp4
```

Run without a path to open the drop target:

```sh
cargo run --release
```

Enzo resumes local, seekable videos from their last saved position and restores the
selected audio and subtitle tracks. It clears the saved entry when playback reaches
the end. Inputs that cannot seek, such as pipes and some URLs, are never saved.

Resume data is stored under `$XDG_STATE_HOME/enzo/watch_later` (or
`~/.local/state/enzo/watch_later`). The store contains only compact state records,
uses private permissions, removes interrupted temporary writes, and limits each
record to 64 KiB. Exact-path records are retained until playback completes or the
store is cleared; moved-file recovery examines at most 512 recent records.

Use `--no-resume` to play without reading or writing resume state:

```sh
cargo run --release -- --no-resume /path/to/video.mp4
```

Use `--clear-resume` to remove all Enzo resume records and exit:

```sh
cargo run --release -- --clear-resume
```

Controls:

- Drop a file or URL on the launcher to play it.
- Space or right click pauses/resumes playback.
- `m` toggles mute.
- `v` toggles subtitles.
- `i` shows media information temporarily; `I` pins or unpins it.
- Left/right arrows seek backward/forward by 5 seconds.
- Down/up arrows seek backward/forward by 60 seconds.
- Click or drag the progress bar to seek.
- `q` quits.

The playback overlay appears while paused, after seeking, and on mouse activity.

Use `--force` for compatible terminals that do not advertise themselves as Kitty.
