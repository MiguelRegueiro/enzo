<h1 align="left"><img src="assets/logo.png" width="64" alt="enzo logo" align="absmiddle" />&nbsp;enzo</h1>

Terminal video player with a graphical interface.

![Enzo playback](assets/screenshots/enzo-night-sky-playback.png)

## Features

- **Playback** — local files and HTTP(S) URLs rendered through Kitty graphics
- **Interface** — timeline, playback controls, track menus, media information, and help
- **Audio** — synchronized playback with volume, mute, and track selection
- **Subtitles** — external and embedded text or bitmap tracks with automatic sidecar detection
- **Resume** — restores position and selected tracks for local seekable files
- **Input** — mouse and keyboard controls, plus a drop-target launcher

## Requirements

- Linux or FreeBSD
- Kitty terminal
- FFmpeg, PulseAudio, FreeType, HarfBuzz, and FriBidi

## Installation

### Fedora

Enable the COPR repository and install with `dnf`:

```bash
sudo dnf copr enable miguelregueiro/enzo
sudo dnf install enzo
```

### Cargo

Install from crates.io:

```bash
cargo install enzo
```

## Run from source

Install Rust 1.96+ and the native development headers for the libraries above, then run:

```sh
cargo run --release
```

Pass a file path or HTTP(S) URL to start playback directly:

```sh
cargo run --release -- /path/to/video.mp4
```

## CLI

```text
enzo [OPTIONS] [VIDEO-OR-URL]
```

Flags:

- `-h`, `--help` — show command-line help and exit
- `-V`, `--version` — show the Enzo version and exit
- `--force` — bypass Kitty terminal detection
- `--force-media-title <title>` — override the displayed title
- `--sub-file <path>` — load an external SRT, WebVTT, SSA, or ASS subtitle file
- `--no-resume` — play without reading or writing resume data
- `--no-autoplay-next` — stop instead of playing the next video when playback ends
- `--clear-resume` — remove saved resume data and exit

<details>
<summary><strong>Controls</strong></summary>

- Drop a file or URL on the launcher to play it
- Space or right click pauses/resumes playback
- `9` / `0` or mouse wheel decreases/increases volume by 2%
- `m` toggles mute
- `a` opens the audio-track menu
- `s` opens the subtitle-track menu
- `v` toggles subtitles
- `i` shows media information; `I` pins or unpins it
- `[` / `]` plays the previous/next video in the same folder
- `?` toggles help; Esc closes open panels
- Left/right arrows seek by 5 seconds
- Down/up arrows seek by 60 seconds, or navigate an open track menu
- Click or drag the progress bar to seek
- Mouse wheel scrolls open audio/subtitle menus
- `q` quits
- `Q` quits without saving resume state

The playback overlay appears while paused, after seeking, and on mouse movement.

</details>
