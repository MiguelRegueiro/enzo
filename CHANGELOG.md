# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Folder playlists with natural ordering, previous/next controls, and autoplay.

### Changed

- The subtitle control now remains visible when no subtitles are available.

### Fixed

- HLS playback with nonstandard segment extensions.
- Audio continuity after seeking HLS streams.
- Accurate HLS seeking across segment boundaries.
- Custom media titles supplied by external launchers.
- Resume playback for titled HLS streams across rotating media URLs.

### Security

- Restricted remote media and nested playlist I/O to read-only HTTP(S), with local-file isolation and bounded FFmpeg operations.

## [1.0.0] - 2026-08-03

### Added

- Initial public release of `enzo`.
- Local file and URL playback with FFmpeg-based media probing and decoding.
- Full-color video output through the Kitty graphics protocol, with adaptive scaling, resize handling, and hardened frame buffering for different terminal and source sizes.
- Synchronized PulseAudio output with audio track selection, pause, volume controls, and mute.
- Mouse and keyboard playback controls, including timeline scrubbing, seek previews, five-second and one-minute seeking, and quitting without saving resume state.
- Graphical overlays with playback controls, a seekable timeline, media title, track menus, status messages, detailed media information, acrylic panels, and responsive help.
- External SRT, WebVTT, and SSA/ASS subtitles with automatic sidecar discovery, explicit `--sub-file` loading, and subtitle loading by drag and drop during playback.
- Embedded text and bitmap subtitle tracks, including PGS rendering, background track loading, language-aware labels, font fallback, and bidirectional text shaping.
- Saved resume state for local seekable media, restoring playback position and audio/subtitle track selection, with options to disable or clear saved state.
- Drop-target launcher for opening local files and URLs when Enzo starts without a media argument.
- Kitty graphics passthrough inside tmux and shared-memory frame transfer for smoother local playback.
- Linux and FreeBSD support, plus Linux desktop entry and application icon assets.
- Command-line help and version output, terminal detection override, explicit subtitle selection, and resume controls.

[Unreleased]: https://github.com/MiguelRegueiro/enzo/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/MiguelRegueiro/enzo/releases/tag/v1.0.0
