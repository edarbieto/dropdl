# Music library mode (planned)

> Status: design, not implemented yet. Current audio behavior is described in [download-rules.md](download-rules.md).

Goal: YouTube Music songs, albums and whole artists become a clean local music library. Every file has complete, consistent tags, a cover and lyrics, so any music player or media server groups it correctly.

## Format

| Option | How | Notes |
|---|---|---|
| **M4A / AAC** (default) | copy YouTube Music's AAC stream (format 140, 128 kbps; 141, 256 kbps, needs a Premium session) | no re-encode, no quality loss |
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

Single songs go to the same layout, using their album when YouTube Music knows it. Multi-disc naming is only used when MusicBrainz provides the discs (see below).

Characters that Windows doesn't allow in names (`< > : " / \ | ? *` and control characters) become `_`, e.g. `CMND/CTRL` → `CMND_CTRL`. Trailing dots and spaces are removed. The tags keep the original text.

## Tag schema

The same fields for every format, written by dropdl after yt-dlp downloads the audio:

| Field | Source / rule |
|---|---|
| Title | track title, without "(Official Audio)"-style suffixes |
| Artist | all track artists, as a multi-value tag. From MusicBrainz artist credits when matched. Without a match: YouTube Music leaves a track's artist empty when it equals the album artist, then the album artist is used |
| Album artist | the album's main artist, **identical on every track** of the album, otherwise players split the album |
| Album | album title |
| Track / total | from the album's track list, not from the playlist index |
| Disc / total | from MusicBrainz. Without a match, 1/1: YouTube Music numbers multi-disc albums as one continuous list (e.g. 1–78) |
| Date | full release date from MusicBrainz, else the year from YouTube Music |
| MusicBrainz IDs | release, release group, recording and artist IDs when matched. Players and servers use them to group reliably |
| Compilation | set for "Various Artists" albums |
| Cover | embedded, square JPEG ~1200 px |
| Lyrics | embedded (see below) |

Why dropdl writes the tags itself: yt-dlp's YouTube Music metadata is incomplete. The album artist is often missing, several artists come as one "A, B" string and track numbers come from the playlist index. dropdl reads the album page from YouTube Music's internal API (the one the web client uses): album artist, year, track list with numbers, and the high-resolution cover.

Tag library: [`lofty`](https://crates.io/crates/lofty) (pure Rust; ID3v2, MP4, Opus/Vorbis; covers and lyrics), about +0.53 MB on the release exe (see Spike results).

## MusicBrainz matching

Before tagging, dropdl looks up the album in [MusicBrainz](https://musicbrainz.org): open data (CC0), free, no API key. If there is no reliable match (common for small or very new artists), only the YouTube Music data is used. Discogs was not chosen because its search API needs a token.

1. Search releases by artist and album title (`/ws/2/release?query=…`).
2. Keep candidates with the same total track count as the YouTube Music album.
3. Compare position by position: every duration within ±3 s and the titles equal after normalizing (lowercase, accents and punctuation removed) for at least 90% of the tracks.
4. Several candidates pass (one album has many editions): prefer Digital Media, then CD, then the earliest date. Vinyl editions split an album into sides, so they come last.
5. No candidate passes: YouTube Music data only.

From a match dropdl takes the disc numbers and totals, the full release date, the titles and artist credits, and the MusicBrainz IDs. The album artist stays the same as long as MusicBrainz credits the same artist. The cover still comes from YouTube Music (always available, square). MusicBrainz allows 1 request per second with a User-Agent that identifies the app. One album needs about two requests.

## Cover and lyrics: embedded, plus one cover.jpg

- **Cover:** embedded in every track, so a file stays complete when copied to a phone or a car. Also written once per album as `cover.jpg`, which servers and file browsers read without opening each track. JPEG rather than PNG, because it is several times smaller.
- **Lyrics:** embedded as text (ID3 `USLT`, MP4 `©lyr`). When synced lyrics are found, the LRC text with timestamps is stored there. Most players and servers read timestamps from this field. Separate `.lrc` files are not written by default: they get lost when files are moved, and they duplicate the data.

Lyrics sources, in order:
1. [LRCLIB](https://lrclib.net): open API, no key, matched by artist, title, album and duration. Synced LRC when available, else its plain lyrics.
2. YouTube Music's own lyrics (plain text only with the web client).
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

1. **Spikes (no UI):** done, see Spike results below.
2. **Format:** done. M4A copies the AAC stream (141 if available, else 140), MP3 is the existing re-encode path, and the audio format is a setting (see download-rules.md).
3. **Metadata module** (new `src/music.rs`): YouTube Music client (album, artist releases, playlist), normalized `Track`/`Album` structs, and offline tests with recorded JSON fixtures (no personal data). Then MusicBrainz matching on top, with fixture tests for a match, a multi-disc match and no match.
4. **Tagging:** after yt-dlp finishes, write the tag schema with `lofty`: embedded cover (square JPEG ~1200 px), lyrics, then `cover.jpg` per album folder. yt-dlp still downloads the audio and the thumbnail. dropdl's tags replace `--embed-metadata` for music.
5. **Library layout and skip-existing:** compute paths from album data before downloading, and skip existing files. Sanitize names and keep path lengths within Windows limits.
6. **Collections:** album → its tracks; artist → albums, singles and EPs queued as album jobs; playlist → album layout or playlist folder (setting).
7. **Lyrics:** LRCLIB synced, then LRCLIB plain, then YouTube Music plain lyrics, else none. A lyrics failure never fails the download.
8. **UI and progress:** show artist/album jobs as "N of M tracks, K skipped". Update the manual checklist in `docs/testing.md`.

Risks:
- The internal API is undocumented and can change. Keep the parsing in one module with fixture tests, and fall back to plain yt-dlp metadata when it fails.
- Rate limiting when downloading large discographies: downloads stay sequential, and lookups are spaced out.

## Spike results

Measured in October 2026 with throwaway code (not in the repo).

**Release size** (variant G, see [release.md](release.md)):

| Build | Size | Delta |
|---|---:|---:|
| current release | 7,174,656 B | |
| + `serde_json` (parse a `Value`) | 7,196,672 B | +22 KB |
| + `serde_json` + `lofty` (MP4 and ID3v2 writing, cover, lyrics) | 7,739,392 B | +551 KB |

Still only Windows system DLLs. `lofty` 0.25.4 does not compile with `default-features = false`, so keep its defaults (`flate2` is already pulled in by `zip`). HTTP stays on `ureq`.

**`lofty` tagging** (checked on generated M4A and MP3 files):
- MP4: title, artist, album, album artist, track, disc, `©lyr` and a 1200×1200 cover are written correctly.
- ID3v2 (MP3): writes ID3v2.4. Several artists become one `TXXX:ARTISTS` frame with null-separated values, and `TPE1` holds the first artist.
- Gotcha: lyrics in ID3v2 need `ItemKey::UnsyncLyrics` (→ `USLT`). `ItemKey::Lyrics` is silently dropped for ID3v2. The language defaults to `XXX`; a native `UnsynchronizedTextFrame` can set it.

**YouTube Music internal API** (`POST https://music.youtube.com/youtubei/v1/{search,browse,next}`, client `WEB_REMIX`, no login needed):
- Album (`browse` with `MPREb_…`): `musicResponsiveHeaderRenderer` has the title, type (Album / EP / Single), year, the album artist with its channel id (`straplineTextOne`) and the cover. The track list (`secondaryContents…musicShelfRenderer`) has the track number (`index`), title, artists, duration and `videoId` per track. `microformat.urlCanonical` gives the album's `OLAK5uy_…` playlist.
- Several artists come as separate runs: artists carry a `browseEndpoint`, separators (`, `, ` & `) don't. Use the runs, not string splitting.
- Cover: replace the URL's size suffix with `=w1200-h1200-…` to get a square 1200 px JPEG directly. No cropping needed.
- No disc numbers: a double album (30 tracks) is numbered 1–30, and "Deftones – The Studio Album Collection" (7 albums) is one album numbered 1–78.
- A track's artist column is empty when it equals the album artist (all 78 tracks of the Deftones collection).
- `OLAK5uy_…` → album: `browse` with `VLOLAK5uy_…` returns the playlist page, which links to the album's `MPREb_…`.
- Artist (`browse` with `UC…`): the "Albums" and "Singles & EPs" shelves link to a discography page (`MPAD…` + `params`) with the complete list (11 albums and 25 singles/EPs for the artist tested). Check continuations on very large discographies.
- Lyrics: `next` with a `videoId` gives a lyrics browse id (`MPLY…`). Its `browse` returns plain text only, no timestamps.

**LRCLIB** (`GET /api/get?artist_name&track_name&album_name&duration`):
- Matches with a duration tolerance of about ±2 s, also without an album name. Returns 404 (`TrackNotFound`) on a miss.
- Out of 4 real tracks (English, Spanish, Japanese): 3 had synced lyrics and 1 only plain lyrics, so LRCLIB's plain lyrics come before YouTube Music's.
- Title differences between YouTube Music and LRCLIB (e.g. "Remastered" or "feat." suffixes) are a likely source of misses. `/api/search` is a possible fallback.

**MusicBrainz** (`GET https://musicbrainz.org/ws/2/release?query=…&fmt=json`, then `/release/<id>?inc=recordings+artist-credits+isrcs+release-groups`):
- "Deftones – The Studio Album Collection": one result, a digital release with 7 discs (11+10+12+11+12+11+11 = 78 tracks). Each disc is titled with the original album (Adrenaline … Koi no Yokan). All 78 tracks match YouTube Music by position, title and duration (±3 s).
- "Daft Punk – Random Access Memories": 27 releases (CD, vinyl, digital, anniversary editions with 22 tracks). The track-count filter and the format preference pick a 13-track single-disc edition.
- Releases include ISRCs and a Cover Art Archive flag.
