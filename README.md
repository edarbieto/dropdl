# dropdl

A minimal desktop app for Windows and macOS that downloads video or audio from a link. Copy a link, press **Ctrl+V** (**⌘V** on a Mac) in the window, and the download starts in the most compatible format.

Built with Rust and [egui](https://github.com/emilk/egui). Under the hood it uses [yt-dlp](https://github.com/yt-dlp/yt-dlp), [deno](https://deno.com) and [ffmpeg](https://ffmpeg.org), which are downloaded automatically on first run.

## Features

- **Paste and go:** pressing Ctrl+V / ⌘V anywhere in the window queues the link right away. You can also type or paste into the box and press Enter.
- **Smart format choice (Auto mode):**
  - YouTube Music, SoundCloud, Bandcamp, Mixcloud and Audiomack links become **M4A** (the original AAC audio, without re-encoding, with cover art and tags). MP3 is available as an option.
  - Everything else (YouTube, TikTok, Instagram, Facebook, Zoom, X, Vimeo, …) becomes **MP4 with H.264 + AAC**, which plays almost anywhere.
  - YouTube Music songs, albums and artists become a music library: `Album Artist/Album (Year)/01 - Title.m4a`, with complete tags (MusicBrainz when a release matches), a square cover and `cover.jpg`. Tracks you already have are skipped.
  - You can force **Video (MP4)** or **Audio** instead.
- **Self-provisioning tools:** on startup the app validates yt-dlp, deno, ffmpeg and ffprobe, downloads whatever is missing, and updates them when a new release exists.
- **Portable single executable:** one ~7 MB `dropdl.exe` with no installer and no DLLs to ship. Tools and settings live in `bin/` and `config/` next to it, so it runs from any folder or USB drive. On macOS: a ~6 MB `dropdl.app` that keeps its data in `~/Library/Application Support/dropdl`.
- **Connect Firefox:** reuses your Firefox login cookies for sites that need a session (Instagram, Facebook, private or age-restricted videos).
- Sequential download queue with progress, cancel, and "show in folder".
- Remembers the output folder, mode, audio format and Firefox connection.

## Usage

1. **Windows:** put `dropdl.exe` in any folder and run it. On first run, it creates `bin/` and `config/` beside itself.
   **macOS:** unzip `dropdl-macos-arm64.zip` and open `dropdl.app`. The app is not notarized: the first time, allow it in System Settings > Privacy & Security ("Open Anyway").
   Wait for the four tool indicators to turn green (the first run downloads about 200 MB of tools).
2. Copy a link and press **Ctrl+V** (**⌘V**) in the window.
3. Click a finished item to reveal the file in Explorer / Finder.

Downloads go to your *Downloads* folder by default. Use **Change…** to pick another folder.

## Building

Requires Rust (stable, edition 2024).

```sh
scripts/build-release.ps1      # Windows → dist/dropdl.exe (portable, dependency-checked)
scripts/build-release.ps1 -Upx # same, compressed with UPX (~2.7 MB)
scripts/bundle-macos.sh        # macOS → dist/dropdl.app + zip (ad-hoc signed)
cargo test                     # unit tests (offline)
cargo test -- --ignored --nocapture   # end-to-end test (network, see docs/testing.md)
```

## Documentation

- [docs/architecture.md](docs/architecture.md): how the code is organized
- [docs/download-rules.md](docs/download-rules.md): which format each link gets, and why
- [docs/tooling.md](docs/tooling.md): data folders, and how tools are validated, installed and updated
- [docs/cookies.md](docs/cookies.md): the "Connect Firefox" feature and troubleshooting
- [docs/testing.md](docs/testing.md): how to test
- [docs/release.md](docs/release.md): portable release build, size measurements and trade-offs
- [docs/music-library.md](docs/music-library.md): music library mode for YouTube Music (in progress)
- [docs/platforms.md](docs/platforms.md): Windows/macOS differences, planned Linux support
- [ROADMAP.md](ROADMAP.md): planned work

## Status

Video and audio downloads work as described above, on Windows and macOS (Apple Silicon). In progress (v0.3): a **music library mode** for YouTube Music. Songs, albums and whole artists already download into a tagged library (`Album Artist/Album (Year)/01 - Title.m4a`) with covers and disc numbers, using MusicBrainz when a release matches. Next: playlists, then lyrics. See [ROADMAP.md](ROADMAP.md).

## Limitations

- Windows (x64) and macOS (Apple Silicon) for now. Intel Macs should work but are untested; Linux is on the roadmap.
- Only Firefox can be connected for login cookies (see docs/cookies.md).
- Zoom recordings protected by a passcode are not supported.
- Some sites block certain networks or require a login. Use **Connect Firefox** in those cases.
- Only download content you have the right to download.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in dropdl by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

dropdl downloads [yt-dlp](https://github.com/yt-dlp/yt-dlp) (Unlicense), [deno](https://deno.com) (MIT) and an ffmpeg build (GPL; [gyan.dev](https://www.gyan.dev/ffmpeg/builds/) on Windows, [martin-riedl.de](https://ffmpeg.martin-riedl.de) on macOS) from their official releases at runtime and runs them as separate programs. They are not part of dropdl and keep their own licenses.
