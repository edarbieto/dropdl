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

## v0.3 (in progress): music library mode

Design and implementation plan: [docs/music-library.md](docs/music-library.md). **Next up: step 5, library layout and skip-existing, which wires in the metadata and tags** (notes in the plan).

- [x] M4A/AAC output without re-encoding (default for all audio), MP3 as an option
- [x] Album metadata from YouTube Music's internal API: songs (not music videos), track artists, cover, artist discographies (not wired into downloads yet)
- [x] MusicBrainz matching (disc numbers, original dates, IDs), falling back to YouTube Music data (not wired into downloads yet)
- [ ] Library layout: `Album Artist/Album (Year)/01 - Title.ext`, plus `cover.jpg` per album
- [x] Complete tags written by dropdl (album artist, multi-value artists, track/disc numbers, date, MusicBrainz IDs) (not wired into downloads yet)
- [x] Embedded square cover (~1200 px JPEG) and `cover.jpg` (not wired into downloads yet)
- [ ] Embedded lyrics: synced from LRCLIB, fallback to YouTube Music's plain lyrics
- [ ] Whole artists: queue every album, single and EP as an album job
- [ ] Playlists: album layout by default, optional playlist folder
- [ ] Skip tracks whose file already exists, checked before downloading

## v0.4: macOS and Linux (macOS done, Linux open)

Design: [docs/platforms.md](docs/platforms.md).

- [x] Per-platform tool downloads (OS + arch, executable bit), yt-dlp as a program folder on macOS, ffmpeg from martin-riedl.de
- [x] ffmpeg/ffprobe auto-update on all platforms (Windows moved from BtbN master builds to gyan.dev release builds)
- [x] Process-group kill, reveal in Finder, fallback fonts, ⌘V hint
- [x] Cookies from Firefox only (Chromium browsers can't be read reliably)
- [x] macOS `.app` (Apple Silicon, ad-hoc signed): `scripts/bundle-macos.sh`
- [ ] Test the Windows changes on a Windows machine (gyan.dev migration, Firefox-only panel)
- [ ] Linux: tool downloads (`.tar.xz` for BtbN), `xdg-open`, Noto CJK fonts, AppImage + ELF tarball
- [ ] Intel Mac test, or a universal binary
- [ ] CI builds on GitHub Actions

## Next

- [ ] Third-party license notices shipped with release builds (see docs/release.md, License notices)
- [ ] App icon (Windows and macOS) and Windows version metadata
- [ ] Clearer error messages for common failures (login required, geo-blocked, rate-limited), with a hint to connect Firefox
- [ ] "Retry" button on failed items
- [ ] Show the total size and speed of the current download

## Later / maybe

- [ ] Accept links dropped as `.url` files, and text drag-and-drop if winit adds support
- [ ] Zoom passcode-protected recordings (passcode prompt)
- [ ] Cookies from an exported `cookies.txt` file (fallback when browser reading fails)
- [ ] Parallel downloads (configurable, default 1)

## Non-goals

- Format pickers, resolution menus or advanced yt-dlp options. The point is one good default (the only exception is a small audio choice: M4A / MP3).
