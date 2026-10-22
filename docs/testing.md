# Testing

## Unit tests (offline)

```sh
cargo test
```

These cover URL parsing, audio/video and single/collection classification, and parsing of yt-dlp progress lines (`src/job.rs`).

## End-to-end test (network)

```sh
cargo test -- --ignored --nocapture
```

`job::e2e::install_and_download`:

1. Runs `tools::ensure_all`, which installs any missing tools into the data folder's `bin/`. By default that is next to the test binary (`target/debug/deps/bin`); set `DROPDL_HOME` to reuse an existing folder.
2. Downloads each test URL in Auto mode, then checks that exactly one file is produced with the expected extension (`.m4a` for audio sites, `.mp4` otherwise).

Environment variables:

| Variable | Default |
|---|---|
| `DROPDL_TEST_URLS` | a short public YouTube video, via youtube.com and music.youtube.com |
| `DROPDL_TEST_OUT` | `%TEMP%\dropdl-test` |
| `DROPDL_HOME` | folder of the test binary |

`tools::tests::latest_versions` (also `--ignored`) checks that the latest yt-dlp and deno versions can be read from GitHub.

Some networks are rate-limited or blocked by YouTube, TikTok or Vimeo ("Sign in to confirm you're not a bot", "IP address is blocked"). In that case, use other public links, e.g.:

```powershell
$env:DROPDL_TEST_URLS = "https://soundcloud.com/ethmusic/lostin-powers-she-so-heavy https://www.bilibili.com/video/BV13x41117TL"
cargo test -- --ignored --nocapture
```

## Manual checklist

- [ ] `scripts/build-release.ps1` passes the dependency check.
- [ ] Copy `dist/dropdl.exe` to an empty folder and run it: `bin/` and `config/` are created and all four indicators go from yellow (downloading) to green.
- [ ] Replace `bin/yt-dlp.exe` with an older release and restart the app: it updates to the latest one.
- [ ] Run from a read-only folder: data falls back to the per-user folder (first Log line).
- [ ] Ctrl+V with a link (focus outside the text box) starts the download immediately.
- [ ] Ctrl+V with non-link text puts it in the text box instead.
- [ ] A YouTube Music song gives one M4A with a square cover, and the Log shows "Not converting audio"; an album gives a folder with numbered tracks.
- [ ] With Audio: MP3, the same song gives one MP3 with a square cover.
- [ ] A YouTube video inside a playlist gives only that video.
- [ ] Cancel stops the download (no leftover yt-dlp process in Task Manager).
- [ ] Clicking a finished item reveals the file in Explorer.
- [ ] Connect browser → Firefox shows "Connected (N cookies found)".
- [ ] Settings survive a restart.
