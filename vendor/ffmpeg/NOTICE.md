# Bundled FFmpeg

AudioCraft includes a separate, unmodified FFmpeg executable so video decoding and MP3/AAC
export work without installing anything. AudioCraft starts it as a subprocess; it is not linked
into the Rust program and it does not change AudioCraft's MIT licence.

- **File:** `bin/ffmpeg.exe` (Windows x86_64)
- **Reported version:** `ffmpeg version N-129166-gae9f4540cb-20261121`
- **Source commit:** `ae9f4540cb` in <https://github.com/FFmpeg/FFmpeg>
- **Build used:** BtbN FFmpeg-Builds, `ffmpeg-master-latest-win64-lgpl.zip`, from
  <https://github.com/BtbN/FFmpeg-Builds/releases/tag/latest>
- **Archive SHA-256:** `cba3cb4055ed2a3cfdd9094976871bf5ad17cd790bc9b39fa9ff3d2afeb786e0`
- **Executable SHA-256:** `674099b77826e5619ad4d953d910e4ad0cc693eae4c881c1a5a7fe97917c0bb2`

## Licence

FFmpeg is © 2000–2026 the FFmpeg developers and is licensed **LGPL-3.0-or-later**. The exact
licence texts shipped with this build are `LICENSE.txt` (GNU Lesser General Public License
version 3) and `COPYING.GPLv3.txt` (GNU General Public License version 3, incorporated by the
LGPL). The build includes other permissively licensed codecs; their notices are inside the
executable's `-L` output (`ffmpeg.exe -L`) and in the upstream source tree.

Corresponding source for this exact build is available from the FFmpeg git commit above and from
<https://ffmpeg.org/download.html>. The build scripts are at
<https://github.com/BtbN/FFmpeg-Builds>. No files from the original Adobe Audition application
are included.
