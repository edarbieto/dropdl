# CLAUDE.md

Guidance for AI coding agents working on this repository.

## Project

dropdl is a minimal egui desktop app (Windows and macOS) that downloads media from a pasted link using yt-dlp. It auto-installs and updates its external tools (yt-dlp, deno, ffmpeg, ffprobe). See README.md for features and docs/ for details.

## Principles

- **Keep it simple and minimal.** One window, few controls, few dependencies. Before adding a setting, ask whether a good default would do.
- **UI text is in English.**
- **Compatibility first:** video → MP4 (H.264 + AAC), audio → M4A (AAC copied without re-encoding) or MP3. Rules live in `src/job.rs` and are documented in `docs/download-rules.md`. Keep the two in sync.
- **Never put personal or machine-specific information** (user names, absolute home paths, emails, installed apps) in code, docs or test fixtures. Use placeholders such as `%LOCALAPPDATA%`.
- **Git:** commit only when the user asks. Never push or rewrite history without explicit permission.
- **Distribution goal:** a single, portable `.exe` that is as small as possible, a small `dropdl.app` on macOS, and later Linux builds. Avoid dependencies that need DLLs, installers or platform-specific system libraries (e.g. prefer rustls over OpenSSL). Check the size impact and license of new dependencies, and regenerate `THIRD-PARTY-LICENSES.txt` (`docs/release.md`).

## Workflow

- **Plan, then execute.** For anything beyond a small fix, first write a short plan (files to change, tests, docs, open decisions) and wait for the user's go-ahead. Then implement it, run the tests, and update the docs in the same change.
- **Spike before designing on assumptions.** When behavior of an external service or tool is unknown, check it with a quick experiment and record the result (e.g. `docs/music-library.md`, Spike results).
- **Ask only when something is unclear** or is a genuine decision (trade-offs, scope, naming the user cares about). Otherwise pick the sensible default, say so, and proceed. Offer a recommended option when asking.
- **Current work:** the v0.3.0 release (first public release): testing on macOS and Windows (including the new icon on Windows), README for users. Checklist in `ROADMAP.md`, publishing steps in `docs/release.md`. The music library mode (v0.3) is done; its design and spike results stay in `docs/music-library.md`.

## Layout

```
src/main.rs    UI (eframe/egui), app state, message handling, job scheduling
src/job.rs     URL classification, yt-dlp argument building, process run + output parsing, cookie test
src/tools.rs   tool sources per platform, validation, download and update (into the bin dir)
src/paths.rs   data folders: bin/ and config/ next to the exe (fallback: per-user data dir; always inside a macOS .app)
src/library.rs music library downloads: Album Artist/Album (Year)/NN - Title.ext, skip existing, per-track download + tagging
src/music.rs   music library metadata: Album/Track, link classification, YouTube Music + MusicBrainz lookup
src/ytm.rs     YouTube Music innertube client and page parsers
src/musicbrainz.rs  MusicBrainz search, release matching, apply to Album
src/tags.rs    writes music tags, cover and lyrics into M4A/MP3 files (lofty), cover.jpg
src/lyrics.rs  lyrics lookup: LRCLIB (exact, then search filtered by length), then YouTube Music
tests/fixtures/     recorded, pruned API responses for offline tests (never from a logged-in session; lyrics replaced by placeholder lines); audio/: silent M4A and MP3 for tag tests
scripts/       build-release.ps1 (dist/dropdl.exe, optional UPX), check-deps.ps1 (PE import check), bundle-macos.sh (dist/dropdl.app + zip), icons.sh (assets/ icons from the SVGs)
assets/        app icon: SVG sources and the generated .ico, .icns and PNG (checked in)
build.rs       Windows only: embeds the exe icon and version metadata (winresource)
docs/          architecture, download rules, tooling, cookies, testing, release, platforms, plans (music-library)
ROADMAP.md     planned work
```

## Commands

```sh
cargo build                     # debug build (shows a console window)
cargo build --release           # release build (no console: windows_subsystem)
scripts/build-release.ps1 [-Upx]   # Windows: portable release in dist/, fails on non-system DLL imports
scripts/bundle-macos.sh            # macOS: dist/dropdl.app (ad-hoc signed) + zip, fails on non-system libraries
cargo test                      # offline unit tests
cargo test -- --ignored --nocapture   # e2e: installs tools + real downloads (network)
```

The e2e tests (`install_and_download`, `cancel`, `library::e2e::song`, `library::e2e::artist`, `library::e2e::playlist`) honor `DROPDL_TEST_URLS` (space-separated), `DROPDL_TEST_OUT` and `DROPDL_HOME` (data folder; set it to reuse an existing `bin/` instead of downloading ~400 MB of tools again). Don't run it against the user's real Downloads folder.

## Conventions

- Background work runs on `std::thread`s and reports to the UI via an `mpsc` channel of `Msg`, followed by `ctx.request_repaint()`. No async runtime.
- Spawn child processes with `tools::command()`, which sets `CREATE_NO_WINDOW` on Windows.
- yt-dlp output is machine-parsed through custom `--progress-template` / `--print` prefixes (`[dl]`, `[pp]`, `[done]`). If you change the templates, update `parse_line` and its tests.
- Cancel kills the whole process tree, because yt-dlp is a launcher that spawns a child process (and ffmpeg): `taskkill /T /F` on Windows; on Unix yt-dlp runs in its own process group, killed with `kill -KILL -<pgid>`.
- Platform-specific code uses `cfg!`/`#[cfg]` inline (`tools::sources`, `kill`, `reveal`, fonts); differences are listed in `docs/platforms.md`. Build and test only for the host target (macOS on a Mac, Windows on Windows); the other platform's code is checked when work continues on that machine. Linux comes later.
- eframe 0.36 API: implement `App::ui(&mut self, ui, frame)` (not `update`). eframe uses the **glow** renderer (default features are off); don't re-enable wgpu without measuring.
- Release profile uses `panic = "abort"`: don't rely on catching panics in threads; return `Result`s.
- On Windows a running `.exe` can't be replaced: tool updates only run when no download is active (on every platform), and failed updates keep the old copy.
- Match the existing style: short doc comments on non-obvious items, no comment noise.

## Gotchas

- YouTube needs a JS runtime (deno), passed via `--js-runtimes deno:<path>`.
- `--no-playlist` is always passed. It only affects links that point to both a video and a playlist. Pure playlist/album links still download fully.
- Cookies come from Firefox only (`job::BROWSER`): Chromium browsers can't be read on Windows (app-bound encryption) and prompt for the Keychain on macOS. `yt-dlp --cookies-from-browser firefox --list-impersonate-targets` loads cookies without network access; `test_cookies` uses this.
- macOS 27 blocks other apps from browser data folders ("Operation not permitted"; yt-dlp says "could not find firefox cookies database"). Only Full Disk Access lifts it, and apps can't request it with a prompt: `job::firefox_blocked` detects it and the UI links to the settings pane. The permission is tied to the code signature, so every ad-hoc build loses it (confirmed).
- egui's default font lacks CJK glyphs, so `add_fallback_fonts` loads system fonts (Windows `Fonts`, macOS `/System/Library/Fonts`).
- On macOS yt-dlp is the program-folder build (`bin/yt-dlp/yt-dlp_macos`): the single-file build unpacks itself on every run (~10 s startup).
- Latest tool versions are read from redirects, not the GitHub API (`tools::version_in`). martin-riedl.de answers `HEAD` with 404: use `GET` without following redirects.
- Without a login, YouTube Music album pages often list music videos instead of the songs. `ytm.rs` takes each song's id from the track menu's "View song credits" entry (`MPTC` + id).
- Innertube continuations need the `visitorData` of the first response. Keep it in memory only, and never record it in fixtures.
- lofty 0.25.4 panics when an MP4 write fails halfway (fatal with `panic = "abort"`), and `TagType::remove_from_path` always fails (opens read-only). `tags::write` tags an in-memory copy and replaces the file via a temp file and a rename; keep it that way.
