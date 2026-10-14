# Roadmap

## v0.1 (current)

- [x] Single-window egui UI, Ctrl+V to queue a link
- [x] Auto-detect and install yt-dlp, deno, ffmpeg, ffprobe
- [x] Auto mode: audio sites → MP3, everything else → MP4 (H.264/AAC)
- [x] YouTube Music songs, albums and playlists (folder per album, track numbers, square cover)
- [x] Manual Video/Audio override
- [x] Connect browser (cookies) with a connection test
- [x] Sequential queue with progress, cancel, reveal in Explorer
- [x] "Update yt-dlp" button
- [x] Persisted settings (output folder, mode, browser)

## Next

- [ ] Single, portable executable: one `.exe` with no installer, no extra DLLs and a static CRT (`+crt-static`), that runs from any folder or USB drive
- [ ] Compress the executable (tune size-focused release flags such as `opt-level = "z"` and `panic = "abort"`, then evaluate UPX, checking startup time and antivirus false positives)
- [ ] App icon and Windows version metadata
- [ ] Keep yt-dlp current automatically (e.g. check weekly on startup)
- [ ] Clearer error messages for common failures (login required, geo-blocked, rate-limited), with a hint to connect a browser
- [ ] "Retry" button on failed items
- [ ] Show the total size and speed of the current download

## Later / maybe

- [ ] Accept links dropped as `.url` files, and text drag-and-drop if winit adds support
- [ ] Zoom passcode-protected recordings (passcode prompt)
- [ ] Cookies from an exported `cookies.txt` file (fallback when browser reading fails)
- [ ] Optional M4A/AAC audio output (no re-encode)
- [ ] Parallel downloads (configurable, default 1)
- [ ] macOS/Linux support (tool download URLs, process kill, reveal-in-folder)
- [ ] Portable mode: keep tools and settings next to the exe when a `portable` marker file exists

## Non-goals

- Format pickers, resolution menus or advanced yt-dlp options. The point is one good default.
