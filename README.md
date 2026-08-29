<h1 align="left"><img src="assets/logo.png" width="64" alt="enzo logo" align="absmiddle" />&nbsp;enzo</h1>

Terminal video player with a graphical interface.

![Enzo playback](assets/screenshots/enzo-night-sky-playback.png)

## Features

- **Playback** — local files and HTTP(S) URLs rendered through Kitty graphics
- **Interface** — timeline, playback controls, track menus, media information, and help
- **Audio** — synchronized playback with volume, mute, and track selection
- **Subtitles** — external and embedded text or bitmap tracks with automatic sidecar detection
- **Resume** — restores position and selected tracks for seekable media
- **Input** — mouse and keyboard controls, plus a drop-target launcher

## Requirements

- Linux or FreeBSD
- Kitty terminal
- PulseAudio, FreeType, HarfBuzz, and FriBidi
- FFmpeg for source builds and distro packages; portable binary releases bundle FFmpeg and dav1d

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
- `--config <path>` — load configuration from a custom path
- `--volume-max <percent>` — set the maximum volume from 100 to 1000 (default: 100)
- `--resume`, `--no-resume` — enable or disable reading and writing resume data
- `--autoplay-next`, `--no-autoplay-next` — enable or disable playing the next video when playback ends
- `--clear-resume` — remove saved resume data and exit

## Configuration

Enzo reads `config.toml` from the platform config directory:

| Platform | Config file |
|---|---|
| Linux / BSD | `$XDG_CONFIG_HOME/enzo/config.toml` or `~/.config/enzo/config.toml` |
| macOS | `$XDG_CONFIG_HOME/enzo/config.toml`, `~/.config/enzo/config.toml`, or `~/Library/Application Support/enzo/config.toml` |

Use `enzo --config FILE` to load a custom path instead. Command-line options override values from the config file.

```toml
volume_max = 200
resume = true
autoplay_next = true
```

`volume_max` accepts values from 100 to 1000. Invalid configuration is reported and Enzo falls back to its built-in defaults. See [`examples/config.toml`](examples/config.toml) for an annotated file.

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
- `p` opens the playlist menu
- Page Up / Page Down play the previous/next video in the same folder
- `?` toggles help; Esc closes open panels
- Left/right arrows seek by 5 seconds
- Down/up arrows seek by 60 seconds; in menus, they move the selection
- In the playlist menu, Page Up/Page Down moves by a page and Home/End jumps to the first/last video
- Click or drag the progress bar to seek
- Mouse wheel adjusts volume; in menus, it scrolls the list
- `q` quits
- `Q` quits without saving resume state

The playback overlay appears while paused, after seeking, and on mouse movement.

</details>
