# dropdl

A minimal Windows desktop app that downloads video or audio from a link. Copy a link, press **Ctrl+V** in the window, and the download starts in the most compatible format.

Built with Rust and [egui](https://github.com/emilk/egui). Under the hood it uses [yt-dlp](https://github.com/yt-dlp/yt-dlp), [deno](https://deno.com) and [ffmpeg](https://ffmpeg.org), which are downloaded automatically on first run.

## Features

- **Paste and go:** pressing Ctrl+V anywhere in the window queues the link right away. You can also type or paste into the box and press Enter.
- **Smart format choice (Auto mode):**
  - YouTube Music, SoundCloud, Bandcamp, Mixcloud and Audiomack links become **M4A** (the original AAC audio, without re-encoding, with cover art and tags). MP3 is available as an option.
  - Everything else (YouTube, TikTok, Instagram, Facebook, Zoom, X, Vimeo, …) becomes **MP4 with H.264 + AAC**, which plays almost anywhere.
  - YouTube Music albums and playlists are saved to their own folder with numbered tracks.
  - You can force **Video (MP4)** or **Audio** instead.
- **Self-provisioning tools:** on startup the app validates yt-dlp, deno, ffmpeg and ffprobe, downloads whatever is missing, and updates yt-dlp and deno when a new release exists.
- **Portable single executable:** one ~7 MB `dropdl.exe` with no installer and no DLLs to ship. Tools and settings live in `bin/` and `config/` next to it, so it runs from any folder or USB drive.
- **Connect browser:** reuses your browser's login cookies for sites that need a session (Instagram, Facebook, private or age-restricted videos).
- Sequential download queue with progress, cancel, and "show in folder".
- Remembers the output folder, mode, audio format and connected browser.

## Usage

1. Put `dropdl.exe` in any folder and run it. On first run, it creates `bin/` and `config/` beside itself; wait for the four tool indicators to turn green.
2. Copy a link and press **Ctrl+V** in the window.
3. Click a finished item to reveal the file in Explorer.

Downloads go to your *Downloads* folder by default. Use **Change…** to pick another folder.

## Building

Requires Rust (stable, edition 2024).

```sh
scripts/build-release.ps1      # → dist/dropdl.exe (portable, dependency-checked)
scripts/build-release.ps1 -Upx # same, compressed with UPX (~2.7 MB)
cargo test                     # unit tests (offline)
cargo test -- --ignored --nocapture   # end-to-end test (network, see docs/testing.md)
```

## Documentation

- [docs/architecture.md](docs/architecture.md): how the code is organized
- [docs/download-rules.md](docs/download-rules.md): which format each link gets, and why
- [docs/tooling.md](docs/tooling.md): data folders, and how tools are validated, installed and updated
- [docs/cookies.md](docs/cookies.md): the "Connect browser" feature and troubleshooting
- [docs/testing.md](docs/testing.md): how to test
- [docs/release.md](docs/release.md): portable release build, size measurements and trade-offs
- [docs/music-library.md](docs/music-library.md): planned music library mode (YouTube Music)
- [docs/platforms.md](docs/platforms.md): planned macOS and Linux support
- [ROADMAP.md](ROADMAP.md): planned work

## Limitations

- Windows only for now. The UI stack (egui + glow) is cross-platform; macOS and Linux are on the roadmap.
- Zoom recordings protected by a passcode are not supported.
- Some sites block certain networks or require a login. Use **Connect browser** in those cases.
- Only download content you have the right to download.
