# Contributing to enzo

Thank you for your interest in contributing to enzo!

Bug reports, feature requests, documentation improvements, and code changes are
welcome.

## Getting Started

Install Rust with [rustup](https://rustup.rs/) and the native development
headers described in [Run from source](README.md#run-from-source), then fork and
clone the repository:

```bash
git clone https://github.com/<your-username>/enzo.git
cd enzo
git checkout -b your-branch-name
```

The repository includes [`rust-toolchain.toml`](rust-toolchain.toml), so rustup
will use the expected toolchain and components automatically.

## Project Structure

```text
.
├── .github/           # CI and release workflows
├── assets/            # Logo and screenshots
├── csrc/              # Native media, audio, subtitle, and text code
├── packaging/         # Distribution packaging files
├── src/
│   ├── app/           # Launcher, playback state, and interaction
│   ├── media/         # Rust media and FFI layer
│   ├── overlay/       # Playback interface and panels
│   ├── resume/        # Resume data and track restoration
│   ├── subtitle/      # Subtitle rendering
│   ├── terminal/      # Terminal lifecycle and Kitty graphics
│   └── main.rs        # Binary entrypoint
├── build.rs           # Native build configuration
├── CHANGELOG.md       # Release notes
└── Cargo.toml         # Package manifest
```

## Development

Build or run enzo:

```bash
cargo build
cargo run --release
```

Pass a media path or URL after `--` to start playback directly:

```bash
cargo run --release -- /path/to/video.mp4
```

## Local Checks

Before opening a pull request, run:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --all-features
cargo build --locked --release --all-features
```

## Pull Requests

Keep pull requests focused and easy to review. Open an issue first for larger
features or behavior changes so the approach can be discussed before
implementation.

Playback behavior can vary by operating system, terminal, tmux setup, media
format, and native dependency versions. Manually test affected behavior and
include the relevant environment and media details in the pull request.

Explain what changed, why it changed, and how it was tested. Add a short entry
under `## [Unreleased]` in [`CHANGELOG.md`](CHANGELOG.md) for user-visible
changes.

## Security

For vulnerability reporting and supported-version policy, see
[`SECURITY.md`](SECURITY.md).
