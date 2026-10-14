# dropdl

A minimal Windows desktop app that downloads video or audio from a link. Copy a link, press **Ctrl+V** in the window, and the download starts in the most compatible format.

Built with Rust and [egui](https://github.com/emilk/egui). Under the hood it uses [yt-dlp](https://github.com/yt-dlp/yt-dlp), [deno](https://deno.com) and [ffmpeg](https://ffmpeg.org), which are downloaded automatically on first run.

## Features

- **Paste and go:** pressing Ctrl+V anywhere in the window queues the link right away. You can also type or paste into the box and press Enter.
- **Smart format choice (Auto mode):**
  - YouTube Music, SoundCloud, Bandcamp, Mixcloud and Audiomack links become **MP3** (best VBR quality, with cover art and tags).
  - Everything else (YouTube, TikTok, Instagram, Facebook, Zoom, X, Vimeo, …) becomes **MP4 with H.264 + AAC**, which plays almost anywhere.
  - YouTube Music albums and playlists are saved to their own folder with numbered tracks.
  - You can force **Video (MP4)** or **Audio (MP3)** instead.
- **Self-provisioning tools:** on startup the app validates yt-dlp, deno, ffmpeg and ffprobe, and downloads whatever is missing.
- **Connect browser:** reuses your browser's login cookies for sites that need a session (Instagram, Facebook, private or age-restricted videos).
- Sequential download queue with progress, cancel, and "show in folder".
- Remembers the output folder, mode and connected browser.

## Usage

1. Run `dropdl.exe`. On first run, wait for the four tool indicators to turn green.
2. Copy a link and press **Ctrl+V** in the window.
3. Click a finished item to reveal the file in Explorer.

Downloads go to your *Downloads* folder by default. Use **Change…** to pick another folder.

## Building

Requires Rust (stable, edition 2024).

```sh
cargo build --release          # → target/release/dropdl.exe
cargo test                     # unit tests (offline)
cargo test -- --ignored --nocapture   # end-to-end test (network, see docs/testing.md)
```

## Documentation

- [docs/architecture.md](docs/architecture.md): how the code is organized
- [docs/download-rules.md](docs/download-rules.md): which format each link gets, and why
- [docs/tooling.md](docs/tooling.md): how external tools are found, validated and installed
- [docs/cookies.md](docs/cookies.md): the "Connect browser" feature and troubleshooting
- [docs/testing.md](docs/testing.md): how to test
- [ROADMAP.md](ROADMAP.md): planned work

## Limitations

- Windows only for now.
- Zoom recordings protected by a passcode are not supported.
- Some sites block certain networks or require a login. Use **Connect browser** in those cases.
- Only download content you have the right to download.
