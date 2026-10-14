# Download rules

The goal is one good default per link: the file should play on Windows, phones, TVs and messaging apps without conversion.

## Audio or video?

| Mode | Result |
|---|---|
| Auto (default) | MP3 if the host is an audio site, otherwise MP4 |
| Video (MP4) | always MP4 |
| Audio (MP3) | always MP3 (extracts the audio track) |

Audio sites (`AUDIO_HOSTS` in `src/job.rs`): `music.youtube.com`, `soundcloud.com`, `bandcamp.com` (including `*.bandcamp.com`), `mixcloud.com`, `audiomack.com`.

## Video: MP4 / H.264 / AAC

Uses yt-dlp's `-t mp4` preset, which prefers H.264 video and AAC audio and merges/remuxes into MP4. Metadata is embedded with `--embed-metadata`.

## Audio: MP3

`-t mp3 --audio-quality 0` gives MP3 at the best VBR quality, plus `--embed-metadata --embed-thumbnail`. For YouTube Music, the thumbnail is cropped to a square so it looks like album art.

MP3 was chosen over M4A for maximum compatibility, even though it re-encodes.

## Single item vs. collection

`--no-playlist` is always passed, so a link to a video *inside* a playlist (`watch?v=…&list=…`, including YouTube Music "radio" mixes) downloads only that item.

A link counts as a **collection** (`is_collection`) when it contains `/playlist`, `/browse/`, or a `list=` without a video id. Collections are downloaded entirely.

## File names

| Case | Template |
|---|---|
| Video | `Title [id].mp4` |
| Video playlist | `Playlist/001 - Title.mp4` |
| YouTube Music song | `Artist - Title.mp3` |
| Album / audio playlist | `Album/01 - Title.mp3` (track number set from playlist index) |
| Other audio | `Title.mp3` |

Titles are truncated (`.120B` / `.150B`) to stay within Windows path limits.

## Other flags

- `--no-mtime`: the file date is the download date, not the upload date.
- `--ffmpeg-location <dir>` and `--js-runtimes deno:<path>`: always use the validated tools.
- `--encoding utf-8`: non-ASCII titles and errors are decoded correctly.
- `--cookies-from-browser <browser>`: only when a browser is connected (see cookies.md).
