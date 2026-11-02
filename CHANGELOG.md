# Changelog

## 0.3.1 (2026-11-01)

### Search

- Search YouTube Music from the text box: type anything that isn't a link and press Enter. Results for songs, albums, artists and playlists show their cover, title, artist, year or length, and an **E** for explicit versions.
- **⬇** adds a result to the queue, always as music library audio. An artist asks first, because it queues all their releases. **Load more** shows the next results, and a line below them shows the running download.
- **➡** opens an album, artist or playlist: an artist's top songs, albums, singles and EPs and playlists, and the songs of each, down to a single song. **Back** returns to the same place in the list, without loading it again.
- Albums show how many songs they have.
- **👁** shows a cover larger.
- **⏵** plays 30 seconds of a song, and an album's most played song. dropdl uses Apple's ready-made preview when it finds the same song, usually in about a second, and otherwise takes the clip from the song itself, which takes several seconds.
- Searches, pages and covers stay in memory only. Preview clips stay in a temp folder until dropdl closes.

### Updates

- When a new dropdl is out, **⬆ Update to …** installs it and restarts. If it can't, because of a read-only folder or a platform without a build, its download page opens. On macOS, Connect Firefox needs Full Disk Access again after each update.

### Changes

- Text without `http://` or `https://` only counts as a link when it has a path (`youtu.be/abc`). Names such as `R.E.M.` are searched, and a bare domain no longer starts a download.

## 0.3.0 (2026-10-26)

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
- Windows: one portable `.exe` (under 8 MB, no installer, no DLLs). Tools and settings live next to it, so it runs from any folder or USB drive.
- macOS: a small `dropdl.app`, signed ad hoc (not notarized: allow it once in System Settings > Privacy & Security).
