# Testing

## Unit tests (offline)

```sh
cargo test
```

These cover URL parsing, audio/video and single/collection classification, and parsing of yt-dlp progress lines (`src/job.rs`), tool download sources per platform, version parsing from redirects and the program-folder install (`src/tools.rs`), loading of saved settings (`src/main.rs`), plus the music metadata: YouTube Music page parsing (`src/ytm.rs`), MusicBrainz candidate order and matching (`src/musicbrainz.rs`), link classification and title normalization (`src/music.rs`).

The music tests read recorded responses from `tests/fixtures/`. They are minified and stripped of `responseContext` (which holds a visitor id), tracking fields and menus (except the "View song credits" entry, which holds the song id). Recorded responses must never come from a logged-in session.

```sh
cargo test music::tests::live -- --ignored --nocapture
```

checks the same lookups against the live YouTube Music and MusicBrainz services (network). To see what dropdl finds for any link (song, album, album playlist or artist; several separated by spaces):

```sh
DROPDL_TEST_MUSIC_URL="https://music.youtube.com/watch?v=zKSsP2084nU" cargo test music::tests::show -- --ignored --nocapture
```

(PowerShell: set `$env:DROPDL_TEST_MUSIC_URL` first.)

It prints each album as `Album artist/Album (Year)` with its tracks, artists, song links and the MusicBrainz match. An artist link prints every release (slow: MusicBrainz allows one request per second).

## End-to-end test (network)

```sh
cargo test -- --ignored --nocapture
```

`job::e2e::install_and_download`:

1. Runs `tools::ensure_all`, which installs any missing tools into the data folder's `bin/`. By default that is next to the test binary (`target/debug/deps/bin`); set `DROPDL_HOME` to reuse an existing folder.
2. Downloads each test URL in Auto mode, then checks that exactly one file is produced with the expected extension (`.m4a` for audio sites, `.mp4` otherwise).

`job::e2e::cancel` starts a long download, kills it once progress arrives, and checks that yt-dlp exited and (on macOS/Linux) that no process of its group is left.

Environment variables:

| Variable | Default |
|---|---|
| `DROPDL_TEST_URLS` | a short public YouTube video, via youtube.com and music.youtube.com |
| `DROPDL_TEST_OUT` | `dropdl-test` in the system temp folder |
| `DROPDL_HOME` | folder of the test binary |

`tools::tests::latest_versions` (also `--ignored`) checks that the latest version of every tool source of the current platform can be read.

Some networks are rate-limited or blocked by YouTube, TikTok or Vimeo ("Sign in to confirm you're not a bot", "IP address is blocked"). In that case, use other public links, e.g.:

```sh
DROPDL_TEST_URLS="https://soundcloud.com/ethmusic/lostin-powers-she-so-heavy https://www.bilibili.com/video/BV13x41117TL" cargo test -- --ignored --nocapture
```

To test an update, replace a tool in `bin/` with an older release (or a script that prints an old version) and run the test again: the Log shows `<tool>: updating <old> → <new>`.

The Windows code can be type-checked from macOS with `cargo check --target x86_64-pc-windows-gnu` (after `rustup target add x86_64-pc-windows-gnu`). The `ring` crate compiles C code for the target, which needs a MinGW toolchain (`brew install mingw-w64`) or `CC_x86_64_pc_windows_gnu=clang` with C headers. This only checks that it compiles; Windows behavior needs a Windows machine.

## Manual checklist

Both platforms:

- [ ] Ctrl+V / ⌘V with a link (focus outside the text box) starts the download immediately.
- [ ] Ctrl+V / ⌘V with non-link text puts it in the text box instead.
- [ ] A YouTube Music song gives one M4A with a square cover, and the Log shows "Not converting audio"; an album gives a folder with numbered tracks.
- [ ] With Audio: MP3, the same song gives one MP3 with a square cover.
- [ ] A YouTube video inside a playlist gives only that video.
- [ ] A title in Chinese, Japanese or Korean renders without boxes.
- [ ] Connect Firefox → Connect shows "Connected (N cookies found)".
- [ ] Settings survive a restart.

Windows:

- [ ] `scripts/build-release.ps1` passes the dependency check.
- [ ] Copy `dist/dropdl.exe` to an empty folder and run it: `bin/` and `config/` are created and all four indicators go from yellow (downloading) to green.
- [ ] With an existing BtbN ffmpeg (version `N-…`) in `bin/`, the app replaces ffmpeg and ffprobe with the gyan.dev build (Log: `ffmpeg: updating N-… → 9.x`).
- [ ] Replace `bin/yt-dlp.exe` with an older release and restart the app: it updates to the latest one.
- [ ] Run from a read-only folder: data falls back to the per-user folder (first Log line).
- [ ] Cancel stops the download (no leftover yt-dlp or ffmpeg process in Task Manager).
- [ ] Clicking a finished item reveals the file in Explorer.

macOS:

- [ ] `scripts/bundle-macos.sh` passes the library check and signs the app.
- [ ] Open `dist/dropdl.app`: tools go to `~/Library/Application Support/dropdl/bin` (first Log line), and nothing is written inside the bundle (`codesign --verify dist/dropdl.app` still passes).
- [ ] A copy of the zip downloaded with a browser opens after "Open Anyway" in System Settings > Privacy & Security.
- [ ] Cancel stops the download (`pgrep -fl yt-dlp` shows nothing).
- [ ] Clicking a finished item reveals the file in Finder.
