# Relinking portable Enzo

The portable Linux executable contains statically linked FFmpeg and dav1d code. This document describes how to rebuild Enzo after modifying either library.

The official portable build uses:

```text
FFmpeg 9.0
SHA-256 7f607a00dd0d28a729d5a4811205812eef01cf6ef6155025febb6f36a9062d52

dav1d 1.5.4
SHA-256 686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5
```

The portable Linux archive contains the exact Enzo source, both dependency source archives, the build script, and configuration records from the official build under `compliance/sources/` and `compliance/build-info/`.

## Build environment

Official Linux artifacts are built on Debian 12 with the Rust toolchain pinned by `rust-toolchain.toml`. The portable builder requires a C toolchain, Cargo, curl, GNU Make, Meson, NASM, Ninja, pkg-config, tar, GnuTLS, PulseAudio, FreeType, HarfBuzz, FriBidi, zlib, and liblzma development files.

On Debian 12:

```sh
apt-get update
apt-get install --yes --no-install-recommends \
    build-essential ca-certificates curl git \
    libfreetype6-dev libfribidi-dev libgnutls28-dev \
    libharfbuzz-dev liblzma-dev libpulse-dev meson nasm \
    ninja-build pkg-config xz-utils zlib1g-dev
```

Install Rust using rustup, then enter the exact versioned Enzo source directory under `compliance/sources/` in the portable archive. The commands below assume that directory is the current working directory.

## Reproduce the official build

Place the matching FFmpeg and dav1d archives in the builder's download directory to avoid downloading them again:

```sh
mkdir -p target/portable-linux/downloads
cp ../ffmpeg-9.0.tar.xz target/portable-linux/downloads/
cp ../dav1d-1.5.4.tar.xz target/portable-linux/downloads/
./packaging/portable/linux/build.sh
```

The executable is written to:

```text
target/portable-linux/cargo/release/enzo
```

Build records are written to:

```text
target/portable-linux/build-info/
```

## Rebuild with modified FFmpeg

Extract the supplied FFmpeg source archive outside `target/portable-linux`, modify it, and pass its directory to the builder:

```sh
tar -xf ../ffmpeg-9.0.tar.xz -C ..
ENZO_FFMPEG_SOURCE=../ffmpeg-9.0 \
    ./packaging/portable/linux/build.sh
```

The builder copies the supplied tree before configuring it, so the original modified tree is not altered. It still forces an LGPL-compatible configuration with GPL and nonfree components disabled.

## Rebuild with modified dav1d

Extract and modify dav1d, then provide its source directory:

```sh
tar -xf ../dav1d-1.5.4.tar.xz -C ..
ENZO_DAV1D_SOURCE=../dav1d-1.5.4 \
    ./packaging/portable/linux/build.sh
```

Both source overrides can be used together.

## Relink against an existing static FFmpeg prefix

A prebuilt prefix can be used instead of rebuilding FFmpeg and dav1d:

```sh
ENZO_FFMPEG_PREFIX=/path/to/ffmpeg-prefix \
    ./packaging/portable/linux/build.sh
```

The prefix must remain at the location recorded in its pkg-config files and contain static versions of all six FFmpeg libraries plus pkg-config metadata for FFmpeg and its private dependencies:

```text
libavformat
libavcodec
libavfilter
libavutil
libswscale
libswresample
```

The final audit rejects shared FFmpeg or dav1d dependencies. The external prefix is never deleted or modified.

## Normal system build

The ordinary build remains separate and uses the FFmpeg supplied by the operating system:

```sh
cargo build --locked --release
```

It does not download or compile FFmpeg or dav1d.
