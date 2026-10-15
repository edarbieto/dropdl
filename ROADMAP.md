# Roadmap

## v0.1 (current)

- [x] Single-window egui UI, Ctrl+V to queue a link
- [x] Auto-detect and install yt-dlp, deno, ffmpeg, ffprobe
- [x] Auto mode: audio sites → MP3, everything else → MP4 (H.264/AAC)
- [x] YouTube Music songs, albums and playlists (folder per album, track numbers, square cover)
- [x] Manual Video/Audio override
- [x] Connect browser (cookies) with a connection test
- [x] Sequential queue with progress, cancel, reveal in Explorer
- [x] Persisted settings (output folder, mode, browser)

## v0.2 (current): portable single executable

- [x] Single, portable `.exe`: static CRT, only Windows system DLLs (verified by `scripts/check-deps.ps1`)
- [x] Portable data: `bin/` and `config/` next to the exe, with a per-user fallback when the folder is read-only
- [x] Startup check of folders, tool contents and versions; auto-update of yt-dlp and deno
- [x] Smaller executable: glow renderer, `opt-level = "s"`, `panic = "abort"` (15.1 MB → 6.9 MB, see docs/release.md)
- [x] Optional UPX compression in `scripts/build-release.ps1 -Upx` (→ 2.7 MB)
- [x] Decided: ship variant G (glow, `opt-level = "s"`, `panic = "abort"`, accesskit kept, no UPX by default)
- [ ] Test on a clean machine (Windows Sandbox / VM without the VC++ Redistributable or a GPU driver)

## Next

- [ ] macOS and Linux builds: per-platform tool download URLs, process-tree kill, reveal-in-folder, fallback fonts
- [ ] App icon and Windows version metadata
- [ ] Clearer error messages for common failures (login required, geo-blocked, rate-limited), with a hint to connect a browser
- [ ] "Retry" button on failed items
- [ ] Show the total size and speed of the current download

## Later / maybe

- [ ] Accept links dropped as `.url` files, and text drag-and-drop if winit adds support
- [ ] Zoom passcode-protected recordings (passcode prompt)
- [ ] Cookies from an exported `cookies.txt` file (fallback when browser reading fails)
- [ ] Optional M4A/AAC audio output (no re-encode)
- [ ] Parallel downloads (configurable, default 1)
- [ ] Update ffmpeg automatically (BtbN's "latest" tag has no version number; compare build dates)

## Non-goals

- Format pickers, resolution menus or advanced yt-dlp options. The point is one good default.
