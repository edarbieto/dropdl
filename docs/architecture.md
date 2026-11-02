# Architecture

dropdl is a single binary. `paths.rs` resolves the data folders (`bin/`, `config/`) before the window opens. `main.rs`, `tools.rs` and `job.rs` are shown below. `library.rs` runs music library downloads (YouTube Music songs, albums, artists and playlists): `music.rs`, `ytm.rs` and `musicbrainz.rs` look up the album, `job.rs` downloads each track, `lyrics.rs` finds lyrics, and `tags.rs` writes the tags. `search.rs` holds search results, opened pages and covers, using `ytm.rs`, and `preview.rs` plays song previews (see music-library.md). `update.rs` replaces dropdl with a newer release (see release.md).

```
┌──────────── main.rs (UI thread) ─────────────┐
│ App state: settings, tool states, jobs, log  │
│ ui(): drain channel → schedule → draw        │
└───────▲───────────────────────────┬──────────┘
        │ Msg (mpsc) + request_repaint│ std::thread::spawn
┌───────┴─────────┐        ┌────────▼─────────┐
│ tools.rs        │        │ job.rs           │
│ ensure_all()    │        │ build_args()     │
│ local()/latest  │        │ run() → yt-dlp   │
│ download/unzip  │        │ parse_line()     │
└─────────────────┘        └──────────────────┘
```

## Flow

1. **Startup:** `main` resolves the data folders (`paths::resolve`). `App::new` loads persisted `Settings` from `config/app.ron`, adds fallback fonts, and spawns `tools::ensure_all` on a thread. Each tool reports `Checking → Ready | Downloading(p) → Ready | Failed`, and log lines (updates, failures) arrive as `Msg::Log`.
2. **Queue:** a link enters through Ctrl+V / ⌘V (`egui::Event::Paste` when no text box has focus), or through the text box + Enter/Download. `enqueue` adds a `Job` in `Queued` state. The list is shown newest first. Other text in the box starts a search (`start_search`), whose results replace the list until closed; `add_hit` queues a result as library audio.
3. **Schedule:** on every frame, if nothing is running and all tools are ready, the oldest queued job starts on a thread. It calls `library::run` for YouTube Music songs and albums in audio mode (`library::handles`), which looks up the album and calls `job::run` once per track, and `job::run` otherwise.
4. **Run:** `job::run` spawns yt-dlp with piped stdout/stderr. Stdout lines are parsed into `Event`s (`Progress`, `Processing`, `Done`, `Log`) and sent to the UI. Stderr is collected; the last `ERROR:` line becomes the failure message.
5. **Finish:** `Msg::JobEnd` sets the status to `Done`, `Failed`, or `Cancelled`.

Cancel sets the job's shared `cancel` flag and kills the running yt-dlp; `library::run` checks the flag between tracks.

## Messages

| Msg | Sent by | Effect |
|---|---|---|
| `Tool(tool, state)` | tools thread | updates the indicator, logs version/path |
| `Job(id, event)` | job thread | progress / title / produced files / log; `Event::Collection` names an album or playlist job (track titles then come with `Progress`), `Event::Skipped` adds a file that already existed, `Event::Enqueue` (artist jobs) queues release jobs with the same mode and audio format |
| `JobEnd(id, result)` | job thread | final status |
| `Cookies(browser, result)` | cookie test thread | connects Firefox if cookies were read |
| `Log(line)` | tools thread | appends to the Log (update notices, download failures) |
| `ToolsChecked` | tools thread | re-enables **Check updates** |
| `Update(event)` | update threads | shows **⬆ Update to …** and its progress; on success closes the app, which starts the new build from `on_exit`, after eframe has saved the settings; on failure opens the release page |
| `Search(id, event)` | search threads | adds a page of results, an opened page or a cover to the open search; other ids are dropped |
| `Preview(id, event)` | preview thread | marks the preview as playing or ended; other ids are dropped |

## Persistence

eframe's `persistence` feature stores `Settings` (output dir, mode, audio format, browser) in `config/app.ron` (`NativeOptions::persistence_path`). `Settings::repair` fixes loaded values: if the saved output folder doesn't exist, e.g. when running from a USB drive on another PC, the default Downloads folder is used, and a browser other than Firefox (offered by older versions) is disconnected.

## Processes

All child processes use `tools::command`, which sets `CREATE_NO_WINDOW` on Windows. Windows release builds use `windows_subsystem = "windows"`, so no console window appears.

yt-dlp is a launcher that starts the real Python process, and it starts ffmpeg too, so Cancel (`job::kill`) ends the whole tree: `taskkill /T /F` on Windows; on macOS/Linux yt-dlp runs in its own process group, which is killed with `kill -KILL -<pgid>`. Platform differences are listed in platforms.md.
