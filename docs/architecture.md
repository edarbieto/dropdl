# Architecture

dropdl is a single binary with three modules.

```
┌──────────── main.rs (UI thread) ─────────────┐
│ App state: settings, tool states, jobs, log  │
│ ui(): drain channel → schedule → draw        │
└───────▲───────────────────────────┬──────────┘
        │ Msg (mpsc) + request_repaint│ std::thread::spawn
┌───────┴─────────┐        ┌────────▼─────────┐
│ tools.rs        │        │ job.rs           │
│ ensure_all()    │        │ build_args()     │
│ find()/version()│        │ run() → yt-dlp   │
│ download/unzip  │        │ parse_line()     │
└─────────────────┘        └──────────────────┘
```

## Flow

1. **Startup:** `App::new` loads persisted `Settings`, adds fallback fonts, and spawns `tools::ensure_all` on a thread. Each tool reports `Checking → Ready | Downloading(p) → Ready | Failed`.
2. **Queue:** a link enters through Ctrl+V (`egui::Event::Paste` when no text box has focus), or through the text box + Enter/Download. `enqueue` adds a `Job` in `Queued` state. The list is shown newest first.
3. **Schedule:** on every frame, if nothing is running and all tools are ready, the oldest queued job starts on a thread that calls `job::run`.
4. **Run:** `job::run` spawns yt-dlp with piped stdout/stderr. Stdout lines are parsed into `Event`s (`Progress`, `Processing`, `Done`, `Log`) and sent to the UI. Stderr is collected; the last `ERROR:` line becomes the failure message.
5. **Finish:** `Msg::JobEnd` sets the status to `Done`, `Failed`, or `Cancelled`.

## Messages

| Msg | Sent by | Effect |
|---|---|---|
| `Tool(tool, state)` | tools thread | updates the indicator, logs version/path |
| `Job(id, event)` | job thread | progress / title / produced files / log |
| `JobEnd(id, result)` | job thread | final status |
| `Cookies(browser, result)` | cookie test thread | connects the browser if cookies were read |
| `Updated(result)` | `yt-dlp -U` thread | logs result, re-reads version |

## Persistence

eframe's `persistence` feature stores `Settings` (output dir, mode, browser) in eframe's app-data location under the app id `dropdl`.

## Processes

All child processes use `tools::command`, which sets `CREATE_NO_WINDOW`. Release builds use `windows_subsystem = "windows"`, so no console window appears.
