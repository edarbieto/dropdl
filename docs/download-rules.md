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

## YouTube Music: library layout

Audio downloads (Auto or Audio mode) of YouTube Music **songs, albums, album playlists** (`OLAK5uy_…`) **and artists** go through the music library mode (`src/library.rs`, design in [music-library.md](music-library.md)):

1. dropdl looks up the album on YouTube Music and MusicBrainz ("Looking up album").
2. Each track gets its final path: `Album Artist/Album (Year)/01 - Title.ext` (`1-01 - Title.ext` on multi-disc albums). If that file exists, the track is skipped without downloading.
3. Each remaining track is downloaded alone, as M4A or MP3 with the flags above, without `--embed-metadata` and `--embed-thumbnail`. dropdl then writes its own tags and the square 1200 px cover, renames the file to its final name, and writes `cover.jpg` once per album.

A song link downloads only that song, into its album's folder. When the lookup fails (e.g. a music video with no album), the link is downloaded as described in the rest of this page. An **artist** link (`/channel/UC…`) queues one album job per album, single and EP (newest first), each handled as above; the artist job itself downloads nothing. Playlist links still use the plain download.

## Single item vs. collection

`--no-playlist` is always passed, so a link to a video *inside* a playlist (`watch?v=…&list=…`, including YouTube Music "radio" mixes) downloads only that item.

A link counts as a **collection** (`is_collection`) when it contains `/playlist`, `/browse/`, or a `list=` without a video id. Collections are downloaded entirely.

## File names

| Case | Template |
|---|---|
| Video | `Title [id].mp4` |
| Video playlist | `Playlist/001 - Title.mp4` |
| YouTube Music song or album (library layout) | `Album Artist/Album (Year)/01 - Title.m4a` |
| YouTube Music song without an album | `Artist - Title.m4a` |
| Other album / audio playlist | `Album/01 - Title.m4a` (track number set from playlist index) |
| Other audio | `Title.m4a` |

Audio files end in `.mp3` instead when the MP3 format is selected.

Titles are truncated (`.120B` / `.150B`) to stay within Windows path limits. Library names are sanitized and cut to 120 bytes per folder or file name (see music-library.md, step 5).

## Other flags

- `--no-mtime`: the file date is the download date, not the upload date.
- `--ffmpeg-location <dir>` and `--js-runtimes deno:<path>`: always use the validated tools.
- `--encoding utf-8`: non-ASCII titles and errors are decoded correctly.
- `--cookies-from-browser firefox`: only when Firefox is connected (see cookies.md).
