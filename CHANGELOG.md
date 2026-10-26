# Changelog

## 0.3.0 (unreleased)

First public release. Builds for Windows (x64) and macOS (Apple Silicon).

### Downloads

- Copy a link and press Ctrl+V (⌘V on a Mac) in the window: the download starts right away. Links queue up and download one at a time, with progress, cancel and "show in folder".
- Auto mode picks the most compatible format: MP4 (H.264 + AAC) for video sites; M4A (the original AAC audio, not re-encoded) for YouTube Music, SoundCloud, Bandcamp, Mixcloud and Audiomack. MP3 is available as an option, and Video or Audio can be forced.
- Connect Firefox to reuse its login cookies for sites that need a session (Instagram, Facebook, private or age-restricted videos), with a connection test.

### Music library

- YouTube Music songs, albums, whole artists and playlists download into `Album Artist/Album (Year)/01 - Title.m4a` in a `dropdl` folder inside your Music folder.
- Songs instead of music videos, complete tags (album artist, track and disc numbers, date, MusicBrainz IDs when a release matches), an embedded square cover and `cover.jpg`.
- Lyrics embedded in each track: synced from LRCLIB when available, otherwise YouTube Music's plain lyrics.
- Tracks you already have are skipped, so downloading an album or playlist again only fetches what is missing.

### Tools and packaging

- yt-dlp, deno, ffmpeg and ffprobe are downloaded on first run and updated automatically when a new release exists.
- Windows: one portable `.exe` (about 7 MB, no installer, no DLLs). Tools and settings live next to it, so it runs from any folder or USB drive.
- macOS: a small `dropdl.app`, signed ad hoc (not notarized: allow it once in System Settings > Privacy & Security).
