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

## v0.3 (next): music library mode

Design: [docs/music-library.md](docs/music-library.md).

- [ ] M4A/AAC output without re-encoding (default for YouTube Music), MP3 as an option
- [ ] Library layout: `Album Artist/Album (Year)/01 - Title.ext`, plus `cover.jpg` per album
- [ ] Complete tags written by dropdl (album artist, multi-value artists, track/disc numbers, date) from YouTube Music album data
- [ ] Embedded square cover (~1200 px JPEG)
- [ ] Embedded lyrics: synced from LRCLIB, fallback to YouTube Music's plain lyrics
- [ ] Whole artists: queue every album, single and EP as an album job
- [ ] Playlists: album layout by default, optional playlist folder
- [ ] Skip tracks whose file already exists, checked before downloading

## v0.4: macOS and Linux

Design: [docs/platforms.md](docs/platforms.md).

- [ ] Per-platform tool downloads (OS + arch, executable bit, `.tar.xz`), ffmpeg source for macOS
- [ ] Process-group kill, reveal-in-folder, fallback fonts
- [ ] macOS `.app` (Apple Silicon, ad-hoc signed), Linux AppImage + ELF tarball
- [ ] CI builds on GitHub Actions

## Next

- [ ] App icon and Windows version metadata
- [ ] Clearer error messages for common failures (login required, geo-blocked, rate-limited), with a hint to connect a browser
- [ ] "Retry" button on failed items
- [ ] Show the total size and speed of the current download

## Later / maybe

- [ ] Accept links dropped as `.url` files, and text drag-and-drop if winit adds support
- [ ] Zoom passcode-protected recordings (passcode prompt)
- [ ] Cookies from an exported `cookies.txt` file (fallback when browser reading fails)
- [ ] Parallel downloads (configurable, default 1)
- [ ] Update ffmpeg automatically (BtbN's "latest" tag has no version number; compare build dates)

## Non-goals

- Format pickers, resolution menus or advanced yt-dlp options. The point is one good default (the only exception is a small audio choice: M4A / MP3).
