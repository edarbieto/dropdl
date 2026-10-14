# CLAUDE.md

Guidance for AI coding agents working on this repository.

## Project

dropdl is a minimal egui desktop app (Windows) that downloads media from a pasted link using yt-dlp. It auto-installs its external tools (yt-dlp, deno, ffmpeg, ffprobe). See README.md for features and docs/ for details.

## Principles

- **Keep it simple and minimal.** One window, few controls, few dependencies. Before adding a setting, ask whether a good default would do.
- **UI text is in English.**
- **Compatibility first:** video → MP4 (H.264 + AAC), audio → MP3. Rules live in `src/job.rs` and are documented in `docs/download-rules.md`. Keep the two in sync.
- **Never put personal or machine-specific information** (user names, absolute home paths, emails, installed apps) in code, docs or test fixtures. Use placeholders such as `%LOCALAPPDATA%`.
- **Git:** commit only when the user asks. Never push or rewrite history without explicit permission.
- **Distribution goal:** a single, portable `.exe` that is as small as possible (see ROADMAP.md). Avoid dependencies that need DLLs or installers next to the binary.

## Layout

```
src/main.rs    UI (eframe/egui), app state, message handling, job scheduling
src/job.rs     URL classification, yt-dlp argument building, process run + output parsing, cookie test
src/tools.rs   tool discovery/validation/download (bin dir: %LOCALAPPDATA%\dropdl\bin)
docs/          architecture, download rules, tooling, cookies, testing
ROADMAP.md     planned work
```

## Commands

```sh
cargo build                     # debug build (shows a console window)
cargo build --release           # release build (no console: windows_subsystem)
cargo test                      # offline unit tests
cargo test -- --ignored --nocapture   # e2e: installs tools + real downloads (network)
```

The e2e test honors `DROPDL_TEST_URLS` (space-separated) and `DROPDL_TEST_OUT`. Don't run it against the user's real Downloads folder.

## Conventions

- Background work runs on `std::thread`s and reports to the UI via an `mpsc` channel of `Msg`, followed by `ctx.request_repaint()`. No async runtime.
- Spawn child processes with `tools::command()`, which sets `CREATE_NO_WINDOW`.
- yt-dlp output is machine-parsed through custom `--progress-template` / `--print` prefixes (`[dl]`, `[pp]`, `[done]`). If you change the templates, update `parse_line` and its tests.
- Cancel kills the whole process tree (`taskkill /T /F`), because `yt-dlp.exe` is a launcher that spawns a child process.
- eframe 0.36 API: implement `App::ui(&mut self, ui, frame)` (not `update`).
- Match the existing style: short doc comments on non-obvious items, no comment noise.

## Gotchas

- YouTube needs a JS runtime (deno), passed via `--js-runtimes deno:<path>`.
- `--no-playlist` is always passed. It only affects links that point to both a video and a playlist. Pure playlist/album links still download fully.
- `yt-dlp --cookies-from-browser X --list-impersonate-targets` loads cookies without network access. `test_cookies` uses this.
- egui's default font lacks CJK glyphs, so `add_fallback_fonts` loads Windows system fonts.
