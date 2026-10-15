# External tools and data folders

## Data folders (portable by default)

On startup, dropdl resolves two folders (`src/paths.rs`):

```
<folder containing dropdl.exe>/
├── dropdl.exe
├── bin/       yt-dlp, deno, ffmpeg, ffprobe
└── config/    app.ron (settings: output folder, mode, connected browser)
```

- They are created if missing and checked for write access, so the whole app can be carried on a USB drive.
- If the exe's folder isn't writable (e.g. `Program Files`), dropdl falls back to the per-user data dir (`%LOCALAPPDATA%\dropdl\{bin,config}` on Windows, `~/.local/share/dropdl` on Linux, `~/Library/Application Support/dropdl` on macOS).
- On macOS, dropdl never writes inside an `.app` bundle, because that would break its signature.
- The environment variable `DROPDL_HOME` overrides the location (useful for tests).

The chosen folder is printed as the first line of the Log.

## Tools

| Tool | Why | Source | Auto-update |
|---|---|---|---|
| yt-dlp | the downloader | `github.com/yt-dlp/yt-dlp` latest release, `yt-dlp.exe` | yes |
| deno | JS runtime that yt-dlp needs to solve YouTube's player challenges | `github.com/denoland/deno` latest release, `deno-x86_64-pc-windows-msvc.zip` | yes |
| ffmpeg | merging streams, remuxing, MP3 encoding, embedding covers/metadata | `github.com/BtbN/FFmpeg-Builds` `latest`, `ffmpeg-master-latest-win64-gpl.zip` | no (only validated) |
| ffprobe | used by yt-dlp's post-processors | same archive as ffmpeg | no |

## Startup check (`tools::ensure_all`)

1. **Contents:** each tool in `bin/` must exist and run successfully with `--version` (`-version` for ffmpeg/ffprobe). The parsed version is shown when you hover the indicator.
2. **Versions:** for yt-dlp and deno, the latest release tag is read from GitHub's `/releases/latest` redirect. This is one small request per tool and doesn't use the API rate limit. If the local version differs, the new release is downloaded. If the check fails (e.g. offline), the existing copy is kept and the Log says so.
3. **Missing tools** are downloaded into `bin/`. ffmpeg and ffprobe always come from the same archive and stay side by side, because yt-dlp gets a single `--ffmpeg-location`.
4. **Fallback:** if a download fails, dropdl keeps the old local copy, or uses a working copy found on `PATH`. If neither exists, the indicator turns red with the error.

Download details:
- Files are written to `*.part` and renamed when complete, so an interrupted download never leaves a broken tool.
- From zip archives, only the needed executables are extracted, and the zip is deleted afterwards.

**Check updates** in the toolbar re-runs the whole check. It is disabled while a download is running, because Windows can't replace a running executable. The ffmpeg archive is the largest download (~200 MB). To refresh ffmpeg, delete it from `bin/` and click **Check updates**.
