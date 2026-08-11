# Third-party notices

The portable Linux build of Enzo includes the following third-party software as statically linked components. Enzo itself remains licensed under the MIT License in `LICENSE`.

## FFmpeg 9.0

FFmpeg is copyright © its contributors; individual copyright notices are retained in the corresponding source archive. The portable build uses FFmpeg 9.0 under the GNU Lesser General Public License, version 2.1 or later.

The build explicitly disables GPL and nonfree components:

```text
CONFIG_GPL=0
CONFIG_NONFREE=0
```

License: `LICENSES/FFmpeg-LGPL-2.1-or-later.txt`

This product includes software developed by the Independent JPEG Group. The corresponding source archive retains FFmpeg's original notices for the IJG-derived files.

Source archive: `ffmpeg-9.0.tar.xz`
Source URL: <https://ffmpeg.org/releases/ffmpeg-9.0.tar.xz>
SHA-256: `7f607a00dd0d28a729d5a4811205812eef01cf6ef6155025febb6f36a9062d52`

A matching Enzo source/relink archive accompanies each portable binary release. It contains the exact FFmpeg source archive, build scripts, configuration records, and instructions needed to rebuild Enzo with a modified FFmpeg. See `packaging/portable/RELINK.md`.

## dav1d 1.5.4

Copyright © 2018–2025 VideoLAN and dav1d authors. All rights reserved.

dav1d is distributed under the BSD 2-Clause License.

License: `LICENSES/dav1d-BSD-2-Clause.txt`

Source archive: `dav1d-1.5.4.tar.xz`
Source URL: <https://downloads.videolan.org/videolan/dav1d/1.5.4/dav1d-1.5.4.tar.xz>
SHA-256: `686616b7c69eb88d44459391ab25cac13b6647a3b288835c5784e71c1514a5c5`

The matching source/relink archive contains the exact dav1d source archive used by the portable build.
