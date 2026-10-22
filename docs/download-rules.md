# Download rules

The goal is one good default per link: the file should play on Windows, phones, TVs and messaging apps without conversion.

## Audio or video?

| Mode | Result |
|---|---|
| Auto (default) | audio if the host is an audio site, otherwise MP4 |
| Video (MP4) | always MP4 |
| Audio | always audio (extracts the audio track) |

The audio format is a separate setting: **M4A** (default) or **MP3**. It applies to every audio download.

Audio sites (`AUDIO_HOSTS` in `src/job.rs`): `music.youtube.com`, `soundcloud.com`, `bandcamp.com` (including `*.bandcamp.com`), `mixcloud.com`, `audiomack.com`.

## Video: MP4 / H.264 / AAC

Uses yt-dlp's `-t mp4` preset, which prefers H.264 video and AAC audio and merges/remuxes into MP4. Metadata is embedded with `--embed-metadata`.

## Audio: M4A or MP3

| Format | Flags | Result |
|---|---|---|
| M4A (default) | `-f "ba[acodec^=mp4a]/ba/b" -x --audio-format m4a` | prefers an AAC stream and copies it without re-encoding (YouTube's 140, or 141 with a Premium session; SoundCloud's `hls_aac`). Sources without AAC are converted to AAC |
| MP3 | `-t mp3 --audio-quality 0` | MP3 at the best VBR quality. Re-encodes unless the source is MP3. For players that don't support M4A |

Both add `--embed-metadata --embed-thumbnail`. For YouTube Music, the thumbnail is cropped to a square so it looks like album art.

## Single item vs. collection

`--no-playlist` is always passed, so a link to a video *inside* a playlist (`watch?v=…&list=…`, including YouTube Music "radio" mixes) downloads only that item.

A link counts as a **collection** (`is_collection`) when it contains `/playlist`, `/browse/`, or a `list=` without a video id. Collections are downloaded entirely.

## File names

| Case | Template |
|---|---|
| Video | `Title [id].mp4` |
| Video playlist | `Playlist/001 - Title.mp4` |
| YouTube Music song | `Artist - Title.m4a` |
| Album / audio playlist | `Album/01 - Title.m4a` (track number set from playlist index) |
| Other audio | `Title.m4a` |

Audio files end in `.mp3` instead when the MP3 format is selected.

Titles are truncated (`.120B` / `.150B`) to stay within Windows path limits.

## Other flags

- `--no-mtime`: the file date is the download date, not the upload date.
- `--ffmpeg-location <dir>` and `--js-runtimes deno:<path>`: always use the validated tools.
- `--encoding utf-8`: non-ASCII titles and errors are decoded correctly.
- `--cookies-from-browser <browser>`: only when a browser is connected (see cookies.md).
