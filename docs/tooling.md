# External tools

| Tool | Why | Source |
|---|---|---|
| yt-dlp | the downloader | `github.com/yt-dlp/yt-dlp` latest release, `yt-dlp.exe` |
| deno | JS runtime that yt-dlp needs to solve YouTube's player challenges | `github.com/denoland/deno` latest release, `deno-x86_64-pc-windows-msvc.zip` |
| ffmpeg | merging streams, remuxing, MP3 encoding, embedding covers/metadata | `github.com/BtbN/FFmpeg-Builds` `latest`, `ffmpeg-master-latest-win64-gpl.zip` |
| ffprobe | used by yt-dlp's post-processors | same archive as ffmpeg |

## Discovery and validation

On startup, `tools::ensure_all` checks each tool in this order:

1. `%LOCALAPPDATA%\dropdl\bin\<tool>.exe` (dropdl's own copy)
2. every directory on `PATH`

A candidate is accepted only if it runs and exits successfully with `--version` (or `-version` for ffmpeg/ffprobe). The version string is shown when you hover the indicator.

ffmpeg and ffprobe must sit in the **same directory**, because yt-dlp receives a single `--ffmpeg-location`. If they were found in different places, both are re-installed into the bin dir.

## Installation

Missing tools are downloaded into `%LOCALAPPDATA%\dropdl\bin`:

- Files are written to a `.part` file first and renamed once complete.
- From zip archives, only the needed `.exe` files are extracted (flat), and the zip is then deleted.
- After installing, the tool is validated again. If it doesn't run, the indicator shows the error and a **Retry** button appears.

The ffmpeg archive is the largest download (~200 MB), so the first run takes a while on slow connections.

## Updating

- **Update yt-dlp** runs `yt-dlp -U`. This only works for the standalone `.exe`. A copy installed via pip on `PATH` must be updated with pip.
- To refresh deno or ffmpeg, delete them from the bin dir and restart the app.
