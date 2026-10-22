# External tools and data folders

## Data folders (portable by default)

On startup, dropdl resolves two folders (`src/paths.rs`):

```
<folder containing dropdl.exe / dropdl>/
├── dropdl.exe
├── bin/       yt-dlp, deno, ffmpeg, ffprobe
└── config/    app.ron (settings: output folder, mode, audio format, Firefox connection)
```

- They are created if missing and checked for write access, so the whole app can be carried on a USB drive.
- If the executable's folder isn't writable (e.g. `Program Files`), dropdl falls back to the per-user data dir (`%LOCALAPPDATA%\dropdl\{bin,config}` on Windows, `~/Library/Application Support/dropdl` on macOS, `~/.local/share/dropdl` on Linux).
- On macOS, dropdl never writes inside an `.app` bundle, because that would break its signature. A `dropdl.app` always uses `~/Library/Application Support/dropdl`.
- The environment variable `DROPDL_HOME` overrides the location (useful for tests).

The chosen folder is printed as the first line of the Log.

## Tools

| Tool | Why | Auto-update |
|---|---|---|
| yt-dlp | the downloader | yes |
| deno | JS runtime that yt-dlp needs to solve YouTube's player challenges | yes |
| ffmpeg | merging streams, remuxing, MP3 encoding, embedding covers/metadata | yes, together with ffprobe |
| ffprobe | used by yt-dlp's post-processors | yes, together with ffmpeg |

Download sources per platform (`tools::sources`):

| Tool | Windows | macOS |
|---|---|---|
| yt-dlp | `github.com/yt-dlp/yt-dlp` latest release, `yt-dlp.exe` | same release, `yt-dlp_macos.zip`, unpacked to `bin/yt-dlp/` (the single-file build takes ~10 s to start, see platforms.md) |
| deno | `github.com/denoland/deno` latest release, `deno-x86_64-pc-windows-msvc.zip` | same release, `deno-aarch64-apple-darwin.zip` (`x86_64` on Intel) |
| ffmpeg, ffprobe | [gyan.dev](https://www.gyan.dev/ffmpeg/builds/) `ffmpeg-release-essentials.zip` (both in one zip, ~115 MB) | [martin-riedl.de](https://ffmpeg.martin-riedl.de) `redirect/latest/macos/arm64/release/ffmpeg.zip` and `ffprobe.zip` (`amd64` on Intel, ~28 MB each) |

All ffmpeg builds are release versions (e.g. 9.0.2), statically linked, GPL, with libx264 and libmp3lame. Until October 2026 Windows used BtbN's `master-latest` build, whose version (`N-12345-g…`) can't be compared with a release; such a copy no longer matches the latest version and is replaced by the gyan.dev build on the next check.

## Startup check (`tools::ensure_all`)

1. **Contents:** each tool in `bin/` must exist and run successfully with `--version` (`-version` for ffmpeg/ffprobe). The parsed version is shown when you hover the indicator.
2. **Versions:** the latest version is read from a redirect, without following it. This is one small request per source and doesn't use GitHub's API rate limit:

   | Source | Request | Redirect target |
   |---|---|---|
   | GitHub (yt-dlp, deno) | `github.com/<repo>/releases/latest` | `…/releases/tag/2026.10.21` (deno: `v2.9.8`) |
   | gyan.dev | the download URL | `…/packages/ffmpeg-9.0.2-essentials_build.zip` |
   | martin-riedl.de | the download URL | `/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip` |

   The first number with a dot in the target is the latest version (`tools::version_in`). It is compared with the local version up to the first `-` (ffmpeg builds append their origin: `9.0.2-essentials_build-www.gyan.dev`). If they differ, the new release is downloaded. If the check fails (e.g. offline), the existing copy is kept and the Log says so.
3. **Missing tools** are downloaded into `bin/`. ffmpeg and ffprobe are always installed and updated together and stay side by side, because yt-dlp gets a single `--ffmpeg-location`.
4. **Fallback:** if a download fails, dropdl keeps the old local copy, or uses a working copy found on `PATH` (on macOS also `/opt/homebrew/bin` and `/usr/local/bin`). If neither exists, the indicator turns red with the error.

Download details:
- Files are written to `*.part` and renamed when complete, so an interrupted download never leaves a broken tool.
- From zip archives, only the needed executables are extracted (executable bit set on macOS/Linux), and the zip is deleted afterwards.
- yt-dlp's macOS program folder is unpacked to `bin/yt-dlp.part/` with its permissions and symlinks, then swapped in place of `bin/yt-dlp/`. The old folder is only removed after the swap.

**Check updates** in the toolbar re-runs the whole check. It is disabled while a download is running: Windows can't replace a running executable, and on every platform a tool shouldn't change under a running download.
