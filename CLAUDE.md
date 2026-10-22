# CLAUDE.md

Guidance for AI coding agents working on this repository.

## Project

dropdl is a minimal egui desktop app (Windows) that downloads media from a pasted link using yt-dlp. It auto-installs its external tools (yt-dlp, deno, ffmpeg, ffprobe). See README.md for features and docs/ for details.

## Principles

- **Keep it simple and minimal.** One window, few controls, few dependencies. Before adding a setting, ask whether a good default would do.
- **UI text is in English.**
- **Compatibility first:** video → MP4 (H.264 + AAC), audio → M4A (AAC copied without re-encoding) or MP3. Rules live in `src/job.rs` and are documented in `docs/download-rules.md`. Keep the two in sync.
- **Never put personal or machine-specific information** (user names, absolute home paths, emails, installed apps) in code, docs or test fixtures. Use placeholders such as `%LOCALAPPDATA%`.
- **Git:** commit only when the user asks. Never push or rewrite history without explicit permission.
- **Distribution goal:** a single, portable `.exe` that is as small as possible, and later macOS/Linux builds. Avoid dependencies that need DLLs, installers or platform-specific system libraries (e.g. prefer rustls over OpenSSL). Check the size impact of new dependencies (`docs/release.md`).

## Layout

```
src/main.rs    UI (eframe/egui), app state, message handling, job scheduling
src/job.rs     URL classification, yt-dlp argument building, process run + output parsing, cookie test
src/tools.rs   tool validation, download and update (into the bin dir)
src/paths.rs   data folders: bin/ and config/ next to the exe (fallback: per-user data dir)
src/music.rs   music library metadata: Album/Track, link classification, YouTube Music + MusicBrainz lookup
src/ytm.rs     YouTube Music innertube client and page parsers
src/musicbrainz.rs  MusicBrainz search, release matching, apply to Album
tests/fixtures/     recorded, pruned API responses for offline tests (never from a logged-in session)
scripts/       build-release.ps1 (dist/dropdl.exe, optional UPX), check-deps.ps1 (PE import check)
docs/          architecture, download rules, tooling, cookies, testing, release, plans (music-library, platforms)
ROADMAP.md     planned work
```

## Commands

```sh
cargo build                     # debug build (shows a console window)
cargo build --release           # release build (no console: windows_subsystem)
scripts/build-release.ps1 [-Upx]   # portable release in dist/, fails on non-system DLL imports
cargo test                      # offline unit tests
cargo test -- --ignored --nocapture   # e2e: installs tools + real downloads (network)
```

The e2e test honors `DROPDL_TEST_URLS` (space-separated), `DROPDL_TEST_OUT` and `DROPDL_HOME` (data folder; set it to reuse an existing `bin/` instead of downloading ~400 MB of tools again). Don't run it against the user's real Downloads folder.

## Conventions

- Background work runs on `std::thread`s and reports to the UI via an `mpsc` channel of `Msg`, followed by `ctx.request_repaint()`. No async runtime.
- Spawn child processes with `tools::command()`, which sets `CREATE_NO_WINDOW`.
- yt-dlp output is machine-parsed through custom `--progress-template` / `--print` prefixes (`[dl]`, `[pp]`, `[done]`). If you change the templates, update `parse_line` and its tests.
- Cancel kills the whole process tree (`taskkill /T /F`), because `yt-dlp.exe` is a launcher that spawns a child process.
- eframe 0.36 API: implement `App::ui(&mut self, ui, frame)` (not `update`). eframe uses the **glow** renderer (default features are off); don't re-enable wgpu without measuring.
- Release profile uses `panic = "abort"`: don't rely on catching panics in threads; return `Result`s.
- On Windows a running `.exe` can't be replaced: tool updates only run when no download is active, and failed updates keep the old copy.
- Match the existing style: short doc comments on non-obvious items, no comment noise.

## Gotchas

- YouTube needs a JS runtime (deno), passed via `--js-runtimes deno:<path>`.
- `--no-playlist` is always passed. It only affects links that point to both a video and a playlist. Pure playlist/album links still download fully.
- `yt-dlp --cookies-from-browser X --list-impersonate-targets` loads cookies without network access. `test_cookies` uses this.
- egui's default font lacks CJK glyphs, so `add_fallback_fonts` loads Windows system fonts.
- Without a login, YouTube Music album pages often list music videos instead of the songs. `ytm.rs` takes each song's id from the track menu's "View song credits" entry (`MPTC` + id).
- Innertube continuations need the `visitorData` of the first response. Keep it in memory only, and never record it in fixtures.
