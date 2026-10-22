# Music library mode (planned)

> Status: design, not implemented yet. Current audio behavior is described in [download-rules.md](download-rules.md).

Goal: YouTube Music songs, albums and whole artists become a clean local music library. Every file has complete, consistent tags, a cover and lyrics, so any music player or media server groups it correctly.

## Format

| Option | How | Notes |
|---|---|---|
| **M4A / AAC** (default for music) | copy YouTube Music's AAC stream (format 140, 128 kbps; 141, 256 kbps, needs a Premium session) | no re-encode, no quality loss |
| MP3 | re-encode, best VBR (`--audio-quality 0`) | maximum compatibility, lossy-to-lossy |
| Opus (maybe later) | copy the Opus stream (~160 kbps) | best source quality, weaker support in some car/hardware players |

The audio choice is a small setting (MP3 / M4A), not a format picker.

## Folder layout

```
<output>/
└── Album Artist/
    └── Album (Year)/
        ├── cover.jpg
        ├── 01 - Title.m4a
        └── 02 - Title.m4a        (multi-disc: 1-01 - Title.m4a)
```

Single songs go to the same layout, using their album when YouTube Music knows it.

## Tag schema

The same fields for every format, written by dropdl after yt-dlp downloads the audio:

| Field | Source / rule |
|---|---|
| Title | track title, without "(Official Audio)"-style suffixes |
| Artist | all track artists, as a multi-value tag |
| Album artist | the album's main artist, **identical on every track** of the album, otherwise players split the album |
| Album | album title |
| Track / total, disc / total | from the album's track list, not from the playlist index |
| Date | release year (full date when known) |
| Compilation | set for "Various Artists" albums |
| Cover | embedded, square JPEG ~1200 px |
| Lyrics | embedded (see below) |

Why dropdl writes the tags itself: yt-dlp's YouTube Music metadata is incomplete. The album artist is often missing, several artists come as one "A, B" string, track numbers come from the playlist index and there is no disc number. dropdl reads the album page from YouTube Music's internal API (the one the web client uses): album artist, year, track list with numbers, and the high-resolution cover.

Candidate tag library: [`lofty`](https://crates.io/crates/lofty) (pure Rust; ID3v2, MP4, Opus/Vorbis; covers and lyrics). Measure its size impact first (docs/release.md).

Later: optional MusicBrainz matching (IDs, genres). Matching recordings reliably is hard, so it is not part of the first version.

## Cover and lyrics: embedded, plus one cover.jpg

- **Cover:** embedded in every track, so a file stays complete when copied to a phone or a car. Also written once per album as `cover.jpg`, which servers and file browsers read without opening each track. JPEG rather than PNG, because it is several times smaller.
- **Lyrics:** embedded as text (ID3 `USLT`, MP4 `©lyr`). When synced lyrics are found, the LRC text with timestamps is stored there. Most players and servers read timestamps from this field. Separate `.lrc` files are not written by default: they get lost when files are moved, and they duplicate the data.

Lyrics sources, in order:
1. [LRCLIB](https://lrclib.net): open API, no key, synced LRC, matched by artist, title, album and duration.
2. YouTube Music's own lyrics (usually plain text).
3. None.

## Collections

| Link | Result |
|---|---|
| Song | one file in its album folder |
| Album (`/browse/MPREb…` or `playlist?list=OLAK5uy_…`) | the whole album |
| Artist (`/channel/UC…`) | every album, single and EP of the artist, each queued as a separate album job |
| Playlist | by default, each track goes into its own album folder (album layout). Option: keep a `Playlist/01 - Title.ext` folder instead |

yt-dlp can't structure an artist page by album (it mixes everything into "Songs"), so dropdl lists the artist's releases through the internal API and queues each album.

## Existing tracks are skipped before downloading

Album data (album artist, album, year, track and disc numbers, title) is known before any audio is fetched, so dropdl computes each track's final path first. If that file already exists, the track is skipped and not downloaded. Downloading an artist or album again only fetches what is missing.

- The check uses the path of the selected format (`.m4a` or `.mp3`). A track that exists only in the other format is downloaded.
- A partial download never counts as existing: yt-dlp writes `*.part` files, and tagging happens before the final rename.
- Known limit: a file that was renamed or moved by hand is not recognized and gets downloaded again.

## Settings

Kept to the minimum:

| Setting | Values | Default |
|---|---|---|
| Audio format | M4A, MP3 | M4A |
| Playlists | album layout, playlist folder | album layout |

## Implementation plan

Each step ends with `cargo test` green and the docs updated (`download-rules.md` follows `job.rs`).

1. **Spikes (no UI):**
   - Measure the release-size impact of `lofty`, and of any HTTP/JSON additions (`serde_json`?) for the internal API.
   - Confirm what the YouTube Music internal API returns for an album (`/browse/MPREb…`), an artist (`/channel/UC…`) and a playlist: album artist, year, track and disc numbers, cover URL, lyrics browse id. Also how an `OLAK5uy_…` playlist maps to its album.
   - Check LRCLIB matching (`/api/get` by artist, title, album, duration) on a few real tracks.
2. **Format:** M4A by copying the AAC stream (prefer 141 if available, else 140), MP3 as the existing re-encode path. Add the audio-format setting.
3. **Metadata module** (new `src/music.rs`): YouTube Music client (album, artist releases, playlist), normalized `Track`/`Album` structs, and offline tests with recorded JSON fixtures (no personal data).
4. **Tagging:** after yt-dlp finishes, write the tag schema with `lofty`: embedded cover (square JPEG ~1200 px), lyrics, then `cover.jpg` per album folder. yt-dlp still downloads the audio and the thumbnail. dropdl's tags replace `--embed-metadata` for music.
5. **Library layout and skip-existing:** compute paths from album data before downloading, and skip existing files. Multi-disc naming `1-01 - Title`. Sanitize names and keep path lengths within Windows limits.
6. **Collections:** album → its tracks; artist → albums, singles and EPs queued as album jobs; playlist → album layout or playlist folder (setting).
7. **Lyrics:** LRCLIB synced, fallback to YouTube Music plain lyrics, else none. A lyrics failure never fails the download.
8. **UI and progress:** show artist/album jobs as "N of M tracks, K skipped". Update the manual checklist in `docs/testing.md`.

Risks:
- The internal API is undocumented and can change. Keep the parsing in one module with fixture tests, and fall back to plain yt-dlp metadata when it fails.
- Rate limiting when downloading large discographies: downloads stay sequential, and lookups are spaced out.
