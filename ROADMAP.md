# Roadmap

## v0.1

- [x] Single-window egui UI, Ctrl+V to queue a link
- [x] Auto-detect and install yt-dlp, deno, ffmpeg, ffprobe
- [x] Auto mode: audio sites → MP3, everything else → MP4 (H.264/AAC)
- [x] YouTube Music songs, albums and playlists (folder per album, track numbers, square cover)
- [x] Manual Video/Audio override
- [x] Connect browser (cookies) with a connection test
- [x] Sequential queue with progress, cancel, reveal in Explorer
- [x] Persisted settings (output folder, mode, browser)

## v0.2: portable single executable

- [x] Single, portable `.exe`: static CRT, only Windows system DLLs (verified by `scripts/check-deps.ps1`)
- [x] Portable data: `bin/` and `config/` next to the exe, with a per-user fallback when the folder is read-only
- [x] Startup check of folders, tool contents and versions; auto-update of yt-dlp and deno
- [x] Smaller executable: glow renderer, `opt-level = "s"`, `panic = "abort"` (15.1 MB → 6.9 MB, see docs/release.md)
- [x] Optional UPX compression in `scripts/build-release.ps1 -Upx` (→ 2.7 MB)
- [x] Decided: ship variant G (glow, `opt-level = "s"`, `panic = "abort"`, accesskit kept, no UPX by default)
- [x] Test on a clean machine (Windows Sandbox / VM without the VC++ Redistributable or a GPU driver)

## v0.3: music library mode

Design and implementation plan: [docs/music-library.md](docs/music-library.md). Done in v0.3.0.

- [x] M4A/AAC output without re-encoding (default for all audio), MP3 as an option
- [x] Album metadata from YouTube Music's internal API: songs (not music videos), track artists, cover, artist discographies
- [x] MusicBrainz matching (disc numbers, original dates, IDs), falling back to YouTube Music data
- [x] Library layout: `Album Artist/Album (Year)/01 - Title.ext`, plus `cover.jpg` per album (songs and albums)
- [x] Complete tags written by dropdl (album artist, multi-value artists, track/disc numbers, date, MusicBrainz IDs)
- [x] Embedded square cover (~1200 px JPEG) and `cover.jpg`
- [x] Embedded lyrics: synced from LRCLIB, fallback to YouTube Music's plain lyrics
- [x] Whole artists: queue every album, single and EP as an album job
- [x] Playlists: songs in their album folders, items without an album (music videos) in a playlist folder
- [x] Skip tracks whose file already exists, checked before downloading

## v0.3.0 release: first public release (in progress)

The code for v0.3 is done (`Cargo.toml` is at 0.3.0). Before tagging `v0.3.0` and publishing builds:

Testing (the manual checklist and e2e tests are in [docs/testing.md](docs/testing.md)):

- [ ] macOS (Apple Silicon): `cargo test`, all e2e tests, the manual checklist with the bundled `dist/dropdl.app`, and a copy of the zip downloaded with a browser (Gatekeeper "Open Anyway")
- [ ] Windows: `cargo test` and the e2e tests on a Windows machine; `scripts/build-release.ps1` passes the dependency check; the manual checklist with `dist/dropdl.exe`, including the changes made on macOS (gyan.dev ffmpeg migration, Firefox-only panel, music library mode: sanitized names, long paths, temporary file rename)
- [ ] Windows clean machine (Windows Sandbox or a VM): first run downloads the tools; SmartScreen's "Windows protected your PC" → "More info" → "Run anyway"
- [ ] Measure the Windows release size again (the music library code and `lofty` added about 0.6 MB on macOS) and update docs/release.md and the README

Release contents:

- [x] Third-party license notices: `THIRD-PARTY-LICENSES.txt` generated from `Cargo.lock` with `cargo-about`, next to `dropdl.exe` and inside `dropdl.app` (see docs/release.md, License notices)
- [ ] App icon (Windows and macOS) and Windows version metadata (file version, product name): done on macOS; check the exe icon, taskbar icon and Properties > Details on Windows (docs/release.md, App icon)
- [x] `CHANGELOG.md` with the v0.3.0 notes (they also become the GitHub release notes)
- [ ] README for people who download it: a screenshot or short GIF, a Download section that links to GitHub Releases, first-launch warnings (SmartScreen, Gatekeeper), and a Privacy note (no telemetry; connects only to the sites you paste, YouTube Music, MusicBrainz, LRCLIB and the tool download sites)

Publishing (steps in [docs/release.md](docs/release.md#publishing-a-release)):

- [ ] Tag `v0.3.0` on `master` and create the GitHub release with `dropdl-windows-x64.exe`, `dropdl-macos-arm64.zip` and `SHA256SUMS.txt`
- [ ] Repository: social preview image, issue templates (bug report asking for OS, dropdl version and the Log)

## v0.4: macOS and Linux (macOS done, Linux open)

Design: [docs/platforms.md](docs/platforms.md).

- [x] Per-platform tool downloads (OS + arch, executable bit), yt-dlp as a program folder on macOS, ffmpeg from martin-riedl.de
- [x] ffmpeg/ffprobe auto-update on all platforms (Windows moved from BtbN master builds to gyan.dev release builds)
- [x] Process-group kill, reveal in Finder, fallback fonts, ⌘V hint
- [x] Cookies from Firefox only (Chromium browsers can't be read reliably)
- [x] macOS `.app` (Apple Silicon, ad-hoc signed): `scripts/bundle-macos.sh`
- [ ] Linux: tool downloads (`.tar.xz` for BtbN), `xdg-open`, Noto CJK fonts, AppImage + ELF tarball
- [ ] Intel Mac test, or a universal binary
- [ ] CI builds on GitHub Actions (release artifacts built from the tagged commit, with checksums and build provenance)

## Next

- [ ] Clearer error messages for common failures (login required, geo-blocked, rate-limited), with a hint to connect Firefox
- [ ] "Retry" button on failed items
- [ ] Show the total size and speed of the current download
- [ ] Better names for YouTube Music items without an album (playlist videos, loose songs): uploads repeat the uploader (`Sean Williams - Sean Williams - Where You Left Me (Official Studio Music Video).m4a`); computing the name in dropdl would allow dropping the uploader and "(Official … Video)" suffixes
- [ ] "Refresh tags" for files downloaded before a feature existed (e.g. lyrics), or when LRCLIB gets lyrics later

## Later / maybe

- [ ] Accept links dropped as `.url` files, and text drag-and-drop if winit adds support
- [ ] Zoom passcode-protected recordings (passcode prompt)
- [ ] Cookies from an exported `cookies.txt` file (fallback when browser reading fails)
- [ ] Parallel downloads (configurable, default 1)

## Non-goals

- Format pickers, resolution menus or advanced yt-dlp options. The point is one good default (the only exception is a small audio choice: M4A / MP3).
