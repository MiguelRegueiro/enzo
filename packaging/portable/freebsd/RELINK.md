# Relinking portable Enzo on FreeBSD

The portable FreeBSD executable contains statically linked FFmpeg and dav1d code. This document describes how to rebuild Enzo after modifying either library.

The official portable build uses:

```text
FFmpeg 9.0
SHA-256 7f607a00dd0d28a729d5a4811205812eef01cf6ef6155025febb6f36a9062d52

dav1d 1.5.4
SHA-256 686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5
```

The portable FreeBSD archive contains the exact Enzo source, both dependency source archives, the build script, and configuration records from the official build under `compliance/sources/` and `compliance/build-info/`.

## Build environment

Official FreeBSD artifacts are built on FreeBSD 15.1 with the Rust toolchain pinned by `rust-toolchain.toml`. The portable builder requires a C toolchain, Cargo, curl, GNU Make, GNU tar, Meson, NASM, Ninja, pkgconf, GnuTLS, PulseAudio, FreeType, HarfBuzz, FriBidi, zlib, and liblzma.

```sh
pkg install rust curl git gmake gtar meson nasm ninja pkgconf python3 \
    gnutls pulseaudio freetype2 harfbuzz fribidi
```

Enter the exact versioned Enzo source directory under `compliance/sources/`. The commands below assume that directory is current.

## Reproduce the official build

```sh
mkdir -p target/portable-freebsd/downloads
cp ../ffmpeg-9.0.tar.xz target/portable-freebsd/downloads/
cp ../dav1d-1.5.4.tar.xz target/portable-freebsd/downloads/
./packaging/portable/freebsd/build.sh
```

The executable is written to `target/portable-freebsd/cargo/release/enzo`; build records are written to `target/portable-freebsd/build-info/`.

## Rebuild with modified FFmpeg

```sh
tar -xf ../ffmpeg-9.0.tar.xz -C ..
ENZO_FFMPEG_SOURCE=../ffmpeg-9.0 \
    ./packaging/portable/freebsd/build.sh
```

The builder copies the supplied tree before configuring it and still forces an LGPL-compatible configuration with GPL and nonfree components disabled.

## Rebuild with modified dav1d

```sh
tar -xf ../dav1d-1.5.4.tar.xz -C ..
ENZO_DAV1D_SOURCE=../dav1d-1.5.4 \
    ./packaging/portable/freebsd/build.sh
```

Both source overrides can be used together.

## Relink against an existing static FFmpeg prefix

```sh
ENZO_FFMPEG_PREFIX=/path/to/ffmpeg-prefix \
    ./packaging/portable/freebsd/build.sh
```

The prefix must remain at the location recorded in its pkg-config files and contain static versions and metadata for `libavformat`, `libavcodec`, `libavfilter`, `libavutil`, `libswscale`, and `libswresample`. The final audit rejects shared FFmpeg or dav1d dependencies. The external prefix is never deleted or modified.

## Normal system build

The ordinary build remains separate and uses FreeBSD packages:

```sh
cargo build --locked --release
```

It does not download or compile FFmpeg or dav1d.
