#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod job;
mod library;
mod lyrics;
mod music;
mod musicbrainz;
mod paths;
mod preview;
mod search;
mod tags;
mod tools;
mod update;
mod ytm;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use serde::{Deserialize, Serialize};

use job::{AudioFormat, BROWSER, Event, Mode, Request};
use search::{Click, Loaded, Play, Search, State};
use tools::{Tool, ToolState};
use ytm::{Hit, Kind};

const LOG_LIMIT: usize = 2000;
/// Side of the enlarged cover.
const ZOOM_SIDE: f32 = 360.0;
const PASTE_KEY: &str = if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" };

fn main() -> eframe::Result {
    let dirs = paths::resolve();
    let options = eframe::NativeOptions {
        persistence_path: Some(dirs.config.join("app.ron")),
        viewport: egui::ViewportBuilder::default()
            .with_title("dropdl")
            .with_inner_size([560.0, 420.0])
            .with_min_inner_size([420.0, 300.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native("dropdl", options, Box::new(|cc| Ok(Box::new(App::new(cc, dirs)))))
}

/// Window and taskbar icon. On macOS the empty icon keeps the bundle's `icon.icns`
/// (eframe would otherwise set its own at runtime).
fn app_icon() -> egui::IconData {
    #[cfg(target_os = "macos")]
    return egui::IconData::default();
    #[cfg(not(target_os = "macos"))]
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon-256.png"))
        .expect("embedded icon is a valid PNG")
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    out_dir: PathBuf,
    mode: Mode,
    audio_format: AudioFormat,
    /// yt-dlp browser name to read cookies from, if connected.
    browser: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            out_dir: dirs::download_dir()
                .or_else(dirs::home_dir)
                .unwrap_or_else(|| PathBuf::from(".")),
            mode: Mode::Auto,
            audio_format: AudioFormat::M4a,
            browser: None,
        }
    }
}

impl Settings {
    /// Fixes values saved by older versions or on another computer.
    fn repair(mut self) -> Self {
        // The saved folder may not exist when the app is run from a USB drive on another PC.
        if !self.out_dir.is_dir() {
            self.out_dir = Settings::default().out_dir;
        }
        // Older versions also offered Chromium browsers.
        if self.browser.as_deref().is_some_and(|b| b != BROWSER) {
            self.browser = None;
        }
        self
    }
}

enum Msg {
    Tool(Tool, ToolState),
    Job(usize, Event),
    JobEnd(usize, Result<(), String>),
    Cookies(String, Result<usize, String>),
    Log(String),
    ToolsChecked,
    Search(u64, search::Event),
    Preview(u64, preview::Event),
    Update(update::Event),
}

/// dropdl's own update, once a newer release exists.
enum AppUpdate {
    Available(String),
    Installing(String, f32),
}

/// What a click on the results asks for.
enum Action {
    Add(Hit),
    Ask(String),
    Dismiss,
    Load,
    Open(Hit),
    Retry(Hit),
    Zoom(String),
    Play(Hit),
    Stop,
}

enum Status {
    Queued,
    Running,
    Processing(String),
    Done,
    Failed(String),
    Cancelled,
}

struct Job {
    id: usize,
    url: String,
    mode: Mode,
    audio_format: AudioFormat,
    status: Status,
    title: String,
    /// Shown instead of the link until the job has a title: the search result's name.
    label: String,
    frac: Option<f32>,
    item: Option<(u32, u32)>,
    files: Vec<PathBuf>,
    /// Album or playlist name; `title` is then the current track.
    collection: String,
    /// Files in `files` that already existed.
    skipped: usize,
    /// Release jobs an artist job added.
    queued: usize,
    pid: Arc<Mutex<Option<u32>>>,
    cancel: Arc<AtomicBool>,
}

impl Job {
    /// What the row shows while it runs: the link, the title, or
    /// "Album · Track" for albums and playlists.
    fn name(&self) -> String {
        match (self.collection.is_empty(), self.title.is_empty()) {
            (true, true) if self.label.is_empty() => self.url.clone(),
            (true, true) => self.label.clone(),
            (true, false) => self.title.clone(),
            (false, true) => self.collection.clone(),
            (false, false) => format!("{} · {}", self.collection, self.title),
        }
    }

    fn done_label(&self) -> String {
        let (n, k) = (self.files.len(), self.skipped);
        if self.queued > 0 {
            return format!("✔ {} — {} releases queued", self.title, self.queued);
        }
        if self.collection.is_empty() {
            return match (n, k) {
                (_, 1..) => format!("✔ {} (already downloaded)", self.name()),
                (0 | 1, _) => format!("✔ {}", self.name()),
                _ => format!("✔ {} ({n} files)", self.name()),
            };
        }
        let tracks = if n == 1 { "1 track".to_string() } else { format!("{n} tracks") };
        match k {
            0 => format!("✔ {} — {tracks}", self.collection),
            _ if k == n => format!("✔ {} — {tracks} (all skipped)", self.collection),
            _ => format!("✔ {} — {tracks} ({k} skipped)", self.collection),
        }
    }
}

enum CookieCheck {
    Idle,
    Testing,
    Result(Result<usize, String>),
}

struct App {
    settings: Settings,
    input: String,
    tools: Vec<(Tool, ToolState)>,
    jobs: Vec<Job>,
    next_id: usize,
    log: Vec<String>,
    show_log: bool,
    show_browser: bool,
    cookie_check: CookieCheck,
    dirs: paths::AppDirs,
    checking: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Open results; they replace the queue until closed.
    search: Option<Search>,
    last_search: u64,
    /// Id of the open search, so background work for an older one stops.
    live_search: Arc<AtomicU64>,
    ytm: Arc<Mutex<ytm::Client>>,
    preview: Option<preview::Preview>,
    last_preview: u64,
    /// Songs that couldn't play, and why.
    no_preview: HashMap<String, String>,
    update: Option<AppUpdate>,
    /// The new build, started once this app has saved and closed.
    relaunch: Option<PathBuf>,
    /// A release page to show, when an update couldn't install itself.
    release_page: Option<String>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, dirs: paths::AppDirs) -> Self {
        let settings = cc
            .storage
            .and_then(|s| eframe::get_value::<Settings>(s, eframe::APP_KEY))
            .unwrap_or_default()
            .repair();
        add_fallback_fonts(&cc.egui_ctx);
        let (tx, rx) = channel();
        let mut app = Self {
            settings,
            input: String::new(),
            tools: Tool::ALL.iter().map(|&t| (t, ToolState::Checking)).collect(),
            jobs: Vec::new(),
            next_id: 0,
            log: Vec::new(),
            show_log: false,
            show_browser: false,
            cookie_check: CookieCheck::Idle,
            dirs,
            checking: false,
            tx,
            rx,
            search: None,
            last_search: 0,
            live_search: Arc::default(),
            ytm: Arc::new(Mutex::new(ytm::Client::new())),
            preview: None,
            last_preview: 0,
            no_preview: HashMap::new(),
            update: None,
            relaunch: None,
            release_page: None,
        };
        preview::clear();
        update::clear();
        app.log(format!("dropdl {} ({} {})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH));
        let kind = if app.dirs.portable { "portable" } else { "per-user" };
        let base = app.dirs.bin.parent().unwrap_or(&app.dirs.bin).display().to_string();
        app.log(format!("data folder ({kind}): {base}"));
        app.check_tools(&cc.egui_ctx);
        app
    }

    /// Validates, installs and updates the tools in the background.
    fn check_tools(&mut self, ctx: &egui::Context) {
        self.checking = true;
        let (tx, repaint, bin) = (self.tx.clone(), ctx.clone(), self.dirs.bin.clone());
        std::thread::spawn(move || {
            let send = |msg| {
                let _ = tx.send(msg);
                repaint.request_repaint();
            };
            tools::ensure_all(&bin, |tool, state| send(Msg::Tool(tool, state)), |line| send(Msg::Log(line)));
            send(Msg::ToolsChecked);
        });
        let (tx, repaint) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(version) = update::check() {
                let _ = tx.send(Msg::Update(update::Event::Available(version)));
                repaint.request_repaint();
            }
        });
    }

    fn updating(&self) -> bool {
        self.relaunch.is_some() || matches!(self.update, Some(AppUpdate::Installing(..)))
    }

    /// Downloads and installs `version` in the background.
    fn start_update(&mut self, ctx: &egui::Context, version: String) {
        self.log(format!("dropdl: updating to {version}"));
        self.update = Some(AppUpdate::Installing(version.clone(), 0.0));
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let send = |event| {
                let _ = tx.send(Msg::Update(event));
                ctx.request_repaint();
            };
            let result = update::install(&version, &mut |p| send(update::Event::Progress(p)));
            send(update::Event::Done(result));
        });
    }

    fn handle_update(&mut self, event: update::Event) {
        match (event, self.update.take()) {
            (update::Event::Available(_), running @ Some(AppUpdate::Installing(..))) => self.update = running,
            (update::Event::Available(version), _) => {
                self.log(format!("dropdl: {version} is available"));
                self.update = Some(AppUpdate::Available(version));
            }
            (update::Event::Progress(p), Some(AppUpdate::Installing(version, _))) => {
                self.update = Some(AppUpdate::Installing(version, p));
            }
            (update::Event::Done(Ok(path)), _) => {
                self.log("dropdl: update installed, restarting".into());
                self.relaunch = Some(path);
            }
            (update::Event::Done(Err(e)), Some(AppUpdate::Installing(version, _))) => {
                self.log(format!("dropdl: update failed: {e}"));
                self.release_page = Some(update::release_page(&version));
                self.update = Some(AppUpdate::Available(version));
            }
            (_, other) => self.update = other,
        }
    }

    /// Shown only when there's something to install. A running download or
    /// preview would be cut off by the restart, so it waits for them.
    fn update_row(&mut self, ui: &mut egui::Ui) {
        match &self.update {
            Some(AppUpdate::Installing(version, p)) => {
                ui.small(format!("Updating to {version}: {:.0}%", p * 100.0));
            }
            Some(AppUpdate::Available(version)) => {
                let version = version.clone();
                let busy = self.preview.is_some()
                    || self.jobs.iter().any(|j| matches!(j.status, Status::Running | Status::Processing(_)));
                let hint = if busy {
                    "Waiting for the download or preview to finish".to_string()
                } else if cfg!(target_os = "macos") && self.settings.browser.is_some() {
                    "Installs it and restarts. Firefox will need Full Disk Access again.".to_string()
                } else {
                    "Installs it and restarts".to_string()
                };
                let button = egui::Button::new(format!("⬆ Update to {version}")).small();
                let response = ui.add_enabled(!busy && !self.checking, button);
                if response.on_hover_text(&hint).on_disabled_hover_text(&hint).clicked() {
                    self.start_update(ui.ctx(), version);
                }
            }
            None => {}
        }
    }

    fn tool_path(&self, tool: Tool) -> Option<PathBuf> {
        self.tools
            .iter()
            .find(|(t, _)| *t == tool)
            .and_then(|(_, s)| s.path().map(PathBuf::from))
    }

    fn job_tools(&self) -> Option<job::Tools> {
        let ffmpeg = self.tool_path(Tool::Ffmpeg)?;
        self.tool_path(Tool::Ffprobe)?;
        Some(job::Tools {
            ytdlp: self.tool_path(Tool::YtDlp)?,
            deno: self.tool_path(Tool::Deno)?,
            ffmpeg: ffmpeg.parent()?.to_path_buf(),
        })
    }

    fn enqueue(&mut self, text: &str) -> bool {
        let Some(url) = job::parse_url(text) else {
            return false;
        };
        self.push_job(url, self.settings.mode, self.settings.audio_format);
        true
    }

    fn push_job(&mut self, url: String, mode: Mode, audio_format: AudioFormat) {
        self.jobs.insert(
            0,
            Job {
                id: self.next_id,
                url,
                mode,
                audio_format,
                status: Status::Queued,
                title: String::new(),
                label: String::new(),
                frac: None,
                item: None,
                files: Vec::new(),
                collection: String::new(),
                skipped: 0,
                queued: 0,
                pid: Arc::default(),
                cancel: Arc::default(),
            },
        );
        self.next_id += 1;
    }

    /// Queues a search result. Results are music, so they always go to the
    /// library as audio, whatever the mode.
    fn add_hit(&mut self, hit: &Hit) {
        self.push_job(hit.url.clone(), Mode::Audio, self.settings.audio_format);
        self.jobs[0].label = hit.title.clone();
    }

    /// Opens results for `query`, on the open tab or else on Songs.
    fn start_search(&mut self, query: String, ctx: &egui::Context) {
        self.stop_preview();
        self.last_search += 1;
        self.live_search.store(self.last_search, Ordering::Relaxed);
        let tab = self.search.as_ref().map_or(Kind::Song, |s| s.tab);
        self.search = Some(Search::new(self.last_search, query, tab));
        self.load_results(tab, ctx);
    }

    fn close_search(&mut self) {
        self.stop_preview();
        self.search = None;
        self.live_search.store(0, Ordering::Relaxed);
    }

    /// Loads a tab's first page, or its next one when it has a token.
    fn load_results(&mut self, kind: Kind, ctx: &egui::Context) {
        let sender = self.search_sender(ctx);
        let Some(s) = self.search.as_mut() else { return };
        let results = s.results.entry(kind).or_default();
        results.state = State::Loading;
        let task = search::Task { id: s.id, query: s.query.clone(), kind, token: results.next.clone() };
        search::spawn(task, self.ytm.clone(), self.live_search.clone(), ctx.clone(), sender);
    }

    /// Opens a result's page, read before or not.
    fn open_hit(&mut self, hit: Hit, ctx: &egui::Context) {
        let Some(s) = self.search.as_mut() else { return };
        s.confirm = None;
        s.stack.push(hit.clone());
        if !matches!(s.details.get(&hit.url), Some(Loaded::Ready(_) | Loaded::Loading)) {
            self.load_detail(hit, ctx);
        }
    }

    fn load_detail(&mut self, hit: Hit, ctx: &egui::Context) {
        let Some(s) = self.search.as_mut() else { return };
        s.details.insert(hit.url.clone(), Loaded::Loading);
        search::spawn_detail(hit, s.id, self.live_search.clone(), self.search_sender(ctx));
    }

    /// Sends the open search's background results to the UI.
    fn search_sender(&self, ctx: &egui::Context) -> impl Fn(search::Event) + Clone + Send + 'static {
        let id = self.search.as_ref().map_or(0, |s| s.id);
        let (tx, repaint) = (self.tx.clone(), ctx.clone());
        move |event| {
            let _ = tx.send(Msg::Search(id, event));
            repaint.request_repaint();
        }
    }

    fn zoom_cover(&mut self, url: String, ctx: &egui::Context) {
        let sender = self.search_sender(ctx);
        let Some(s) = self.search.as_mut() else { return };
        if !s.zooms.contains_key(&url) {
            search::spawn_zoom(url.clone(), ctx.clone(), sender);
        }
        s.zoom = Some(url);
    }

    fn play_preview(&mut self, song: Hit, ctx: &egui::Context) {
        self.stop_preview();
        self.last_preview += 1;
        let id = self.last_preview;
        let (tx, repaint) = (self.tx.clone(), ctx.clone());
        self.preview = Some(preview::Preview::start(id, song, self.job_tools(), move |event| {
            let _ = tx.send(Msg::Preview(id, event));
            repaint.request_repaint();
        }));
    }

    fn stop_preview(&mut self) {
        if let Some(preview) = self.preview.take() {
            preview.stop();
        }
    }

    /// Adds a line to the Log, with paths in the home folder shortened to `~` (see `display_path`).
    fn log(&mut self, line: String) {
        self.log.push(tilde_in(&line, dirs::home_dir().as_deref()));
        if self.log.len() > LOG_LIMIT {
            self.log.drain(..self.log.len() - LOG_LIMIT);
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Tool(tool, state) => {
                    match &state {
                        ToolState::Ready { path, version } => {
                            self.log(format!("{} {version} at {}", tool.name(), path.display()))
                        }
                        ToolState::Failed(e) => self.log(format!("{}: {e}", tool.name())),
                        _ => {}
                    }
                    if let Some(slot) = self.tools.iter_mut().find(|(t, _)| *t == tool) {
                        slot.1 = state;
                    }
                }
                // Release jobs keep the artist job's mode and audio format.
                Msg::Job(id, Event::Enqueue(links)) => {
                    let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) else { continue };
                    j.queued = links.len();
                    let (mode, audio_format) = (j.mode, j.audio_format);
                    for url in links {
                        self.push_job(url, mode, audio_format);
                    }
                }
                Msg::Job(id, event) => {
                    let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) else { continue };
                    match event {
                        Event::Progress { frac, item, title } => {
                            j.status = Status::Running;
                            j.frac = frac;
                            if item.is_some() {
                                j.item = item;
                            }
                            if !title.is_empty() && title != "NA" {
                                j.title = title;
                            }
                        }
                        Event::Processing(step) => j.status = Status::Processing(step),
                        Event::Done(path) => {
                            j.files.push(path);
                            j.status = Status::Running;
                        }
                        Event::Skipped(path) => {
                            j.files.push(path);
                            j.skipped += 1;
                        }
                        Event::Collection(name) => j.collection = name,
                        Event::Log(line) => self.log(line),
                        Event::Enqueue(_) => {}
                    }
                }
                Msg::JobEnd(id, result) => {
                    let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) else { continue };
                    j.status = match result {
                        _ if j.cancel.load(Ordering::Relaxed) => Status::Cancelled,
                        Ok(()) => Status::Done,
                        Err(e) => Status::Failed(e),
                    };
                }
                Msg::Cookies(browser, result) => {
                    if result.is_ok() {
                        self.settings.browser = Some(browser);
                    }
                    self.cookie_check = CookieCheck::Result(result);
                }
                Msg::Log(line) => self.log(line),
                Msg::ToolsChecked => self.checking = false,
                Msg::Search(id, event) => {
                    let Some(s) = self.search.as_mut().filter(|s| s.id == id) else { continue };
                    match event {
                        search::Event::Page(kind, page) => {
                            let error = page.as_ref().err().cloned();
                            s.add_page(kind, page);
                            if let Some(e) = error {
                                self.log(e);
                            }
                        }
                        search::Event::Thumb(url, texture) => {
                            s.thumbs.insert(url, texture);
                        }
                        search::Event::Detail(url, detail) => {
                            let error = detail.as_ref().err().cloned();
                            s.details.insert(url, detail.map_or_else(Loaded::Failed, Loaded::Ready));
                            if let Some(e) = error {
                                self.log(e);
                            }
                        }
                        search::Event::Zoom(url, texture) => {
                            s.zooms.insert(url, texture);
                        }
                    }
                }
                Msg::Update(event) => self.handle_update(event),
                Msg::Preview(id, event) => {
                    let Some(p) = self.preview.as_mut().filter(|p| p.id == id) else { continue };
                    match event {
                        preview::Event::Playing => p.playing = true,
                        preview::Event::Ended(result) => {
                            if let Err(e) = result {
                                self.no_preview.insert(p.url.clone(), e.clone());
                                self.log(e);
                            }
                            self.preview = None;
                        }
                    }
                }
            }
        }
    }

    /// Starts the next queued job if nothing is running.
    fn schedule(&mut self, ctx: &egui::Context) {
        let busy = self
            .jobs
            .iter()
            .any(|j| matches!(j.status, Status::Running | Status::Processing(_)));
        if busy || self.updating() {
            return;
        }
        let Some(tools) = self.job_tools() else { return };
        // Oldest first: jobs are stored newest first.
        let Some(j) = self.jobs.iter_mut().rev().find(|j| matches!(j.status, Status::Queued)) else {
            return;
        };
        j.status = Status::Running;
        let in_library = library::handles(&j.url, j.mode);
        let req = Request {
            url: j.url.clone(),
            mode: j.mode,
            audio_format: j.audio_format,
            out_dir: if in_library { library::root(&self.settings.out_dir) } else { self.settings.out_dir.clone() },
            browser: self.settings.browser.clone(),
            template: None,
        };
        let (id, pid, cancel, tx, ctx) = (j.id, j.pid.clone(), j.cancel.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let send = |event| {
                let _ = tx.send(Msg::Job(id, event));
                ctx.request_repaint();
            };
            let result = if in_library {
                library::run(&req, &tools, &pid, &cancel, send)
            } else {
                job::run(&req, &tools, &pid, send)
            };
            let _ = tx.send(Msg::JobEnd(id, result));
            ctx.request_repaint();
        });
    }

    fn test_browser(&mut self, ctx: &egui::Context) {
        let Some(ytdlp) = self.tool_path(Tool::YtDlp) else { return };
        self.cookie_check = CookieCheck::Testing;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = job::test_cookies(&ytdlp, |line| {
                let _ = tx.send(Msg::Log(line));
            });
            let _ = tx.send(Msg::Cookies(BROWSER.to_string(), result));
            ctx.request_repaint();
        });
    }

    // ---- UI ----

    fn tools_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            for (tool, state) in &self.tools {
                let (color, hint) = match state {
                    ToolState::Checking => (ui.visuals().weak_text_color(), "checking…".to_string()),
                    ToolState::Downloading(p) => {
                        (ui.visuals().warn_fg_color, format!("downloading {:.0}%", p * 100.0))
                    }
                    ToolState::Ready { version, .. } => (egui::Color32::from_rgb(80, 180, 100), version.clone()),
                    ToolState::Failed(e) => (ui.visuals().error_fg_color, e.clone()),
                };
                ui.label(egui::RichText::new("●").color(color))
                    .on_hover_text(&hint);
                ui.label(tool.name()).on_hover_text(hint);
                ui.add_space(4.0);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Replacing a binary that a download is using would fail.
                let busy = self.preview.is_some()
                    || self.jobs.iter().any(|j| matches!(j.status, Status::Running | Status::Processing(_)));
                if ui
                    .add_enabled(!self.checking && !busy && !self.updating(), egui::Button::new("Check updates").small())
                    .on_hover_text("Check for a new dropdl, and re-check and update the tools")
                    .clicked()
                {
                    self.check_tools(ui.ctx());
                }
                let label = if self.settings.browser.is_some() { "🍪 Firefox" } else { "Connect Firefox" };
                if ui.small_button(label).clicked() {
                    self.show_browser = !self.show_browser;
                }
            });
        });
        self.update_row(ui);

        let installing: Vec<_> = self
            .tools
            .iter()
            .filter_map(|(t, s)| match s {
                ToolState::Downloading(p) => Some((t.name(), *p)),
                _ => None,
            })
            .collect();
        if let Some((name, p)) = installing.first() {
            ui.add(egui::ProgressBar::new(*p).text(format!("Downloading {name}…")));
        }
    }

    fn browser_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label("Use your Firefox login for sites that need it (Instagram, Facebook, private videos).");
            ui.label(
                egui::RichText::new("Log in to the site in Firefox first. Other browsers keep their cookies locked.")
                    .weak()
                    .small(),
            );
            ui.horizontal(|ui| {
                let testing = matches!(self.cookie_check, CookieCheck::Testing);
                let label = if self.settings.browser.is_some() { "Test again" } else { "Connect" };
                if ui.add_enabled(!testing, egui::Button::new(label)).clicked() {
                    self.test_browser(ui.ctx());
                }
                if self.settings.browser.is_some() && ui.button("Disconnect").clicked() {
                    self.settings.browser = None;
                    self.cookie_check = CookieCheck::Idle;
                }
            });
            match &self.cookie_check {
                CookieCheck::Idle => {}
                CookieCheck::Testing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Reading cookies…");
                    });
                }
                CookieCheck::Result(Ok(n)) => {
                    ui.label(format!("Connected ({n} cookies found)."));
                }
                CookieCheck::Result(Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                    if e == job::FULL_DISK_ACCESS_HINT && ui.button("Open Privacy Settings").clicked() {
                        open_privacy_settings();
                    }
                }
            }
        });
    }

    fn input_row(&mut self, ui: &mut egui::Ui) {
        let ready = self.job_tools().is_some();
        ui.horizontal(|ui| {
            let button_w = 90.0;
            let edit = egui::TextEdit::singleline(&mut self.input)
                .hint_text(format!("Paste a link ({PASTE_KEY}) or search music"))
                .desired_width(ui.available_width() - button_w - ui.spacing().item_spacing.x);
            let resp = ui.add(edit);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let is_link = job::parse_url(&self.input).is_some();
            let query = self.input.trim().to_string();
            let label = if is_link || query.is_empty() { "Download" } else { "Search" };
            let clicked = ui
                .add_enabled(!query.is_empty(), egui::Button::new(label).min_size([button_w, 0.0].into()))
                .clicked();
            if (clicked || enter) && is_link {
                let text = std::mem::take(&mut self.input);
                self.enqueue(&text);
                self.close_search();
            } else if (clicked || enter) && !query.is_empty() {
                self.start_search(query, ui.ctx());
            }
        });
        if !ready {
            ui.label(egui::RichText::new("Preparing tools… downloads will start when ready.").weak().small());
        }
    }

    fn options(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Mode:");
            ui.radio_value(&mut self.settings.mode, Mode::Auto, "Auto")
                .on_hover_text("Audio sites (YouTube Music, SoundCloud, …) → audio, everything else → MP4");
            ui.radio_value(&mut self.settings.mode, Mode::Video, "Video (MP4)");
            ui.radio_value(&mut self.settings.mode, Mode::Audio, "Audio");
        });
        ui.horizontal(|ui| {
            ui.label("Audio:");
            ui.radio_value(&mut self.settings.audio_format, AudioFormat::M4a, "M4A")
                .on_hover_text("Keeps the original AAC audio when possible (no quality loss)");
            ui.radio_value(&mut self.settings.audio_format, AudioFormat::Mp3, "MP3")
                .on_hover_text("Re-encoded, for players that don't support M4A");
        });
        ui.horizontal(|ui| {
            ui.label("Save to:");
            if ui
                .link(display_path(&self.settings.out_dir))
                .on_hover_text("Open folder")
                .clicked()
            {
                open_path(&self.settings.out_dir);
            }
            if ui.small_button("Change…").clicked() {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_directory(&self.settings.out_dir)
                    .pick_folder()
                {
                    self.settings.out_dir = dir;
                }
            }
        });
        let library = library::root(&self.settings.out_dir);
        if library != self.settings.out_dir {
            ui.horizontal(|ui| {
                ui.label("Music library:");
                if ui
                    .link(display_path(&library))
                    .on_hover_text("YouTube Music audio downloads go here. Open folder")
                    .clicked()
                {
                    let _ = std::fs::create_dir_all(&library);
                    open_path(&library);
                }
            });
        }
    }

    fn search_tabs(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.search.as_mut() else { return };
        let (mut open, mut close, mut back) = (None, false, false);
        ui.horizontal(|ui| {
            if s.stack.is_empty() {
                for (kind, name) in search::TABS {
                    if ui.selectable_label(s.tab == kind, name).clicked() {
                        open = Some(kind);
                    }
                }
            } else {
                back = ui.button("⏴ Back").clicked();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.small_button("✖ Close").on_hover_text("Back to the queue").clicked();
                let path: Vec<&str> = s.stack.iter().map(|h| h.title.as_str()).collect();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(path.join(" › ")).weak()).truncate());
                });
            });
        });
        if back {
            s.stack.pop();
            s.confirm = None;
        }
        if let Some(kind) = open {
            s.tab = kind;
            s.confirm = None;
            if s.results.get(&kind).is_none_or(|r| r.state == State::Idle) {
                self.load_results(kind, ui.ctx());
            }
        }
        if close {
            self.close_search();
        }
    }

    /// A result's row. Asks for its action through `action`, and for its page
    /// through `wanted` when it needs it for the song count.
    fn result_row(&self, ui: &mut egui::Ui, hit: &Hit, opened: bool, action: &mut Option<Action>, wanted: &mut Vec<Hit>) {
        let Some(s) = self.search.as_ref() else { return };
        let detail = s.detail(&hit.url);
        let summary = detail.map_or("", |d| d.summary.as_str());
        // A song plays itself and an album its most played song.
        let song = match hit.kind {
            Kind::Song if !hit.url.is_empty() => Some(hit),
            Kind::Album => detail.and_then(|d| d.top.as_ref()),
            _ => None,
        };
        let live = song.and_then(|song| self.preview.as_ref().filter(|p| p.url == song.url));
        let play = match (hit.kind, song, live) {
            (_, _, Some(p)) if p.playing => Play::Playing,
            (_, _, Some(_)) => Play::Loading,
            (Kind::Song, Some(song), _) | (Kind::Album, Some(song), _) => match self.no_preview.get(&song.url) {
                Some(why) => Play::Off(why.clone()),
                None if hit.kind == Kind::Album => Play::Ready(format!("Preview: {}", song.title)),
                None => Play::Ready("Preview".into()),
            },
            (Kind::Album, None, _) => Play::Off("Reading the album…".into()),
            _ => Play::None,
        };
        let shown = search::Row {
            thumb: hit.thumb.as_ref().and_then(|t| s.thumbs.get(t)),
            added: self.jobs.iter().any(|j| j.url == hit.url),
            asking: s.confirm.as_ref() == Some(&hit.url),
            play,
            // The list shows the count; the opened page all of it.
            note: if opened { summary } else { summary.split(" · ").next().unwrap_or_default() },
            open: !opened && hit.kind != Kind::Song,
        };
        let (visible, click) = search::row(ui, hit, shown);
        if visible && hit.kind == Kind::Album && !s.details.contains_key(&hit.url) {
            wanted.push(hit.clone());
        }
        match click {
            Some(Click::Add) => *action = Some(Action::Add(hit.clone())),
            Some(Click::Ask) => *action = Some(Action::Ask(hit.url.clone())),
            Some(Click::No) => *action = Some(Action::Dismiss),
            Some(Click::Open) => *action = Some(Action::Open(hit.clone())),
            Some(Click::Zoom) => *action = hit.thumb.clone().map(Action::Zoom),
            Some(Click::Play) => *action = song.cloned().map(Action::Play),
            Some(Click::Stop) => *action = Some(Action::Stop),
            None => {}
        }
    }

    /// The open page: its own row, then its lists.
    fn page(&self, ui: &mut egui::Ui, opened: &Hit, action: &mut Option<Action>, wanted: &mut Vec<Hit>) {
        let Some(s) = self.search.as_ref() else { return };
        if opened.kind != Kind::Shelf {
            self.result_row(ui, opened, true, action, wanted);
            ui.separator();
        }
        match s.details.get(&opened.url) {
            None | Some(Loaded::Loading) => {
                ui.vertical_centered(|ui| ui.spinner());
            }
            Some(Loaded::Failed(e)) => {
                ui.vertical_centered(|ui| {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                    if ui.button("Try again").clicked() {
                        *action = Some(Action::Retry(opened.clone()));
                    }
                });
            }
            Some(Loaded::Ready(detail)) => {
                for section in &detail.sections {
                    if !section.title.is_empty() {
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(&section.title).strong());
                    }
                    for hit in &section.hits {
                        self.result_row(ui, hit, false, action, wanted);
                    }
                    if let Some(more) = &section.more
                        && ui.small_button("See all").clicked()
                    {
                        *action = Some(Action::Open(more.clone()));
                    }
                }
            }
        }
    }

    fn search_list(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.search.as_ref() else { return };
        let (mut action, mut wanted) = (None, Vec::new());
        if let Some(opened) = s.stack.last() {
            self.page(ui, opened, &mut action, &mut wanted);
        } else if let Some(results) = s.results.get(&s.tab) {
            for hit in &results.hits {
                self.result_row(ui, hit, false, &mut action, &mut wanted);
            }
            ui.add_space(4.0);
            ui.vertical_centered(|ui| match &results.state {
                State::Idle => {}
                State::Loading => {
                    ui.spinner();
                }
                State::Ready if results.hits.is_empty() => {
                    ui.label(egui::RichText::new("No results.").weak());
                }
                State::Ready => {
                    if results.next.is_some() && ui.button("Load more").clicked() {
                        action = Some(Action::Load);
                    }
                }
                State::Failed(e) => {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                    if ui.button("Try again").clicked() {
                        action = Some(Action::Load);
                    }
                }
            });
        }
        let ctx = ui.ctx().clone();
        let free = self.search.as_ref().map_or(0, |s| search::MAX_DETAILS.saturating_sub(s.loading_details()));
        for hit in wanted.into_iter().take(free) {
            self.load_detail(hit, &ctx);
        }
        self.load_page_covers(&ctx);
        match action {
            Some(Action::Add(hit)) => {
                self.add_hit(&hit);
                self.search.iter_mut().for_each(|s| s.confirm = None);
            }
            Some(Action::Ask(url)) => self.search.iter_mut().for_each(|s| s.confirm = Some(url.clone())),
            Some(Action::Dismiss) => self.search.iter_mut().for_each(|s| s.confirm = None),
            Some(Action::Load) => {
                let tab = self.search.as_ref().map(|s| s.tab);
                tab.into_iter().for_each(|tab| self.load_results(tab, &ctx));
            }
            Some(Action::Open(hit)) => self.open_hit(hit, &ctx),
            Some(Action::Retry(hit)) => self.load_detail(hit, &ctx),
            Some(Action::Zoom(url)) => self.zoom_cover(url, &ctx),
            Some(Action::Play(song)) => self.play_preview(song, &ctx),
            Some(Action::Stop) => self.stop_preview(),
            None => {}
        }
    }

    /// Loads the covers of the open page, once. Search pages load their own.
    fn load_page_covers(&mut self, ctx: &egui::Context) {
        let sender = self.search_sender(ctx);
        let Some(s) = self.search.as_mut() else { return };
        let Some(url) = s.stack.last().map(|h| h.url.clone()) else { return };
        let Some(Loaded::Ready(detail)) = s.details.remove(&url) else { return };
        let urls = s.missing_thumbs(detail.sections.iter().flat_map(|section| &section.hits));
        s.details.insert(url, Loaded::Ready(detail));
        if !urls.is_empty() {
            search::fetch_thumbs(urls, s.id, self.live_search.clone(), ctx.clone(), sender);
        }
    }

    /// The enlarged cover. Its small copy stands in until the large one loads,
    /// and for good if that fails.
    fn zoom_dialog(&mut self, ctx: &egui::Context) {
        let Some(s) = self.search.as_mut() else { return };
        let Some(url) = s.zoom.clone() else { return };
        let large = s.zooms.get(&url);
        let texture = large.and_then(Option::as_ref).or(s.thumbs.get(&url));
        let loading = large.is_none();
        let modal = egui::Modal::new(egui::Id::new("cover")).show(ctx, |ui| {
            let side = ui.ctx().content_rect().size().min_elem().min(ZOOM_SIDE + 48.0) - 48.0;
            let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::click());
            match texture {
                Some(texture) => {
                    egui::Image::new(texture).corner_radius(3.0).paint_at(ui, rect);
                }
                None => {
                    ui.painter().rect_filled(rect, 3.0, ui.visuals().widgets.inactive.bg_fill);
                }
            }
            if loading {
                ui.put(egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(24.0)), egui::Spinner::new());
            }
            response.clicked()
        });
        if modal.inner || modal.should_close() {
            s.zoom = None;
        }
    }

    /// The running download and the queue's length, for while results hide the queue.
    fn queue_strip(&self, ui: &mut egui::Ui) {
        let waiting = self.jobs.iter().filter(|j| matches!(j.status, Status::Queued)).count();
        let active = self.jobs.iter().find(|j| matches!(j.status, Status::Running | Status::Processing(_)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if waiting > 0 {
                ui.label(egui::RichText::new(format!("+{waiting} queued")).small());
            }
            let Some(j) = active else { return };
            let item = j.item.map(|(i, n)| format!(" [{i}/{n}]")).unwrap_or_default();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| match &j.status {
                Status::Processing(step) => {
                    ui.spinner();
                    ui.add(egui::Label::new(format!("{step}{item}  {}", j.name())).truncate());
                }
                _ => {
                    let frac = j.frac.unwrap_or(0.0);
                    ui.add(egui::ProgressBar::new(frac).text(format!("{:.0}%{item}  {}", frac * 100.0, j.name())));
                }
            });
        });
    }

    fn job_list(&mut self, ui: &mut egui::Ui) {
        let mut remove = None;
        let out_dir = self.settings.out_dir.clone();
        for j in &mut self.jobs {
            ui.horizontal(|ui| {
                let name = j.name();
                let kind = if job::wants_audio(&j.url, j.mode) { j.audio_format.label() } else { "MP4" };
                let item = j.item.map(|(i, n)| format!(" [{i}/{n}]")).unwrap_or_default();

                let active = matches!(j.status, Status::Running | Status::Processing(_));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if active {
                        if ui.small_button("✖").on_hover_text("Cancel").clicked() {
                            j.cancel.store(true, Ordering::Relaxed);
                            if let Some(pid) = *j.pid.lock().unwrap() {
                                job::kill(pid);
                            }
                        }
                    } else if ui.small_button("🗙").on_hover_text("Remove from list").clicked() {
                        remove = Some(j.id);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(kind).small().weak());
                        match &j.status {
                            Status::Queued => {
                                ui.label(format!("{name} — queued"));
                            }
                            Status::Running => {
                                let frac = j.frac.unwrap_or(0.0);
                                ui.add(
                                    egui::ProgressBar::new(frac)
                                        .text(format!("{:.0}%{item}  {name}", frac * 100.0)),
                                );
                            }
                            Status::Processing(step) => {
                                ui.spinner();
                                ui.label(format!("{step}{item}  {name}"));
                            }
                            Status::Done => {
                                if ui.link(j.done_label()).on_hover_text("Show in folder").clicked() {
                                    match j.files.first() {
                                        Some(f) => reveal(f),
                                        None if library::handles(&j.url, j.mode) => {
                                            open_path(&library::root(&out_dir))
                                        }
                                        None => open_path(&out_dir),
                                    }
                                }
                            }
                            Status::Failed(e) => {
                                let name = if j.collection.is_empty() { &name } else { &j.collection };
                                ui.colored_label(ui.visuals().error_fg_color, format!("✖ {name}"))
                                    .on_hover_text(e);
                            }
                            Status::Cancelled => {
                                let name = if j.collection.is_empty() { &name } else { &j.collection };
                                ui.label(egui::RichText::new(format!("{name} — cancelled")).weak());
                            }
                        }
                    });
                });
            });
            if let Status::Failed(e) = &j.status {
                ui.label(egui::RichText::new(e).small().color(ui.visuals().error_fg_color));
            }
        }
        if let Some(id) = remove {
            self.jobs.retain(|j| j.id != id);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.handle_messages();
        let ctx = ui.ctx().clone();
        if let Some(url) = self.release_page.take() {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        if self.relaunch.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Ctrl+V (⌘V) anywhere outside the text box downloads the clipboard link right away.
        if !ctx.egui_wants_keyboard_input() {
            let pasted = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Paste(s) => Some(s.clone()),
                    _ => None,
                })
            });
            if let Some(text) = pasted {
                if !self.enqueue(&text) {
                    self.input = text;
                }
            }
        }
        self.schedule(&ctx);

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            self.tools_bar(ui);
            if self.show_browser {
                self.browser_panel(ui);
            }
            ui.add_space(6.0);
            self.input_row(ui);
            if self.search.is_some() {
                self.search_tabs(ui);
            } else {
                self.options(ui);
            }
            ui.separator();

            let log_h = if self.show_log { 140.0 } else { 0.0 };
            // Each results tab keeps its own scroll position.
            let area = self.search.as_ref().map(|s| (s.tab, s.stack.last().map(|h| h.url.clone())));
            egui::ScrollArea::vertical()
                .id_salt(("jobs", area))
                .max_height((ui.available_height() - log_h - 28.0).max(60.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if self.search.is_some() {
                        self.search_list(ui);
                        return;
                    }
                    if self.jobs.is_empty() {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(format!("Copy a link and press {PASTE_KEY} here.")).weak());
                        });
                    }
                    self.job_list(ui);
                });

            ui.separator();
            ui.horizontal(|ui| {
                let arrow = if self.show_log { "⏷" } else { "⏵" };
                if ui.small_button(format!("{arrow} Log")).clicked() {
                    self.show_log = !self.show_log;
                }
                if self.show_log && ui.small_button("Copy").clicked() {
                    ui.ctx().copy_text(self.log.join("\n"));
                }
                if self.search.is_some() {
                    self.queue_strip(ui);
                }
            });
            if self.show_log {
                egui::ScrollArea::vertical()
                    .id_salt("log")
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in &self.log {
                            ui.label(egui::RichText::new(line).monospace().small());
                        }
                    });
            }
        });
        self.zoom_dialog(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_preview();
        preview::clear();
        // eframe has saved the settings by now, so the new build reads them.
        if let Some(path) = &self.relaunch
            && let Err(e) = update::relaunch(path)
        {
            eprintln!("could not start {}: {e}", path.display());
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }
}

/// egui's built-in font has no CJK/Korean glyphs; borrow them from the system
/// so titles in those scripts don't render as boxes.
fn add_fallback_fonts(ctx: &egui::Context) {
    let fonts: Vec<PathBuf> = if cfg!(windows) {
        let dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or("C:\\Windows".into())).join("Fonts");
        ["msyh.ttc", "malgun.ttf"].iter().map(|f| dir.join(f)).collect()
    } else if cfg!(target_os = "macos") {
        ["Hiragino Sans GB.ttc", "AppleSDGothicNeo.ttc"]
            .iter()
            .map(|f| PathBuf::from("/System/Library/Fonts").join(f))
            .collect()
    } else {
        Vec::new()
    };
    let mut defs = egui::FontDefinitions::default();
    for path in fonts {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        defs.font_data.insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(name.clone());
        }
    }
    ctx.set_fonts(defs);
}

/// Opens a folder in Explorer / Finder.
fn open_path(path: &std::path::Path) {
    let program = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = tools::command(program).arg(path).spawn();
}

/// Opens System Settings at Privacy & Security > Full Disk Access (macOS).
fn open_privacy_settings() {
    let _ = tools::command("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles")
        .spawn();
}

/// `path` with the home folder shortened to `~`, so screenshots and logs don't show the user name.
fn display_path(path: &Path) -> String {
    tilde(path, dirs::home_dir().as_deref())
}

fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// Replaces every path inside `home` in `text` with `~`. Only `home` followed by a separator
/// matches, so `/Users/ana` doesn't change `/Users/anabel`.
fn tilde_in(text: &str, home: Option<&Path>) -> String {
    let Some(home) = home else { return text.to_string() };
    let sep = std::path::MAIN_SEPARATOR;
    text.replace(&format!("{}{sep}", home.display()), &format!("~{sep}"))
}

/// Shows `file` selected in Explorer / Finder.
fn reveal(file: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = tools::command("explorer")
            .raw_arg(format!("/select,\"{}\"", file.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    let _ = tools::command("open").arg("-R").arg(file).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    open_path(file.parent().unwrap_or(file));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_settings() {
        let saved = |browser: &str| Settings {
            out_dir: std::env::temp_dir(),
            browser: Some(browser.into()),
            ..Settings::default()
        };
        assert_eq!(saved("chrome").repair().browser, None);
        assert_eq!(saved("firefox").repair().browser.as_deref(), Some("firefox"));
        let missing = Settings { out_dir: "/no/such/folder".into(), ..Settings::default() };
        assert_eq!(missing.repair().out_dir, Settings::default().out_dir);
    }

    #[test]
    fn tilde_paths() {
        let home = Path::new("/home/user");
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(tilde(&home.join("Downloads"), Some(home)), format!("~{sep}Downloads"));
        assert_eq!(tilde(home, Some(home)), "~");
        assert_eq!(tilde(Path::new("/home/username/x"), Some(home)), "/home/username/x");
        assert_eq!(tilde(Path::new("/media/usb"), Some(home)), "/media/usb");
        assert_eq!(tilde(Path::new("/media/usb"), None), "/media/usb");
    }

    #[test]
    fn tilde_log_lines() {
        let home = Path::new("/home/user");
        let s = std::path::MAIN_SEPARATOR;
        let line = format!("-P /home/user{s}Music{s}dropdl --ffmpeg-location /home/user{s}bin /home/username{s}x");
        assert_eq!(
            tilde_in(&line, Some(home)),
            format!("-P ~{s}Music{s}dropdl --ffmpeg-location ~{s}bin /home/username{s}x")
        );
        assert_eq!(tilde_in(&line, None), line);
    }

    /// An app without a window, and a sender for its messages.
    #[test]
    fn update_states() {
        let (mut app, tx) = test_app();
        let send = |e| tx.send(Msg::Update(e)).unwrap();

        send(update::Event::Available("9.9.9".into()));
        app.handle_messages();
        assert!(matches!(&app.update, Some(AppUpdate::Available(v)) if v == "9.9.9"));
        assert!(!app.updating());

        app.update = Some(AppUpdate::Installing("9.9.9".into(), 0.0));
        // A check that ends during the install doesn't bring the button back.
        send(update::Event::Available("9.9.9".into()));
        send(update::Event::Progress(0.5));
        app.handle_messages();
        assert!(matches!(app.update, Some(AppUpdate::Installing(_, p)) if p == 0.5));
        assert!(app.updating());

        // A failure shows the download page and allows another try.
        send(update::Event::Done(Err("read-only folder".into())));
        app.handle_messages();
        assert!(matches!(&app.update, Some(AppUpdate::Available(_))));
        assert_eq!(app.release_page.as_deref(), Some("https://github.com/edarbieto/dropdl/releases/tag/v9.9.9"));
        assert!(app.log.iter().any(|l| l.contains("read-only folder")));

        app.update = Some(AppUpdate::Installing("9.9.9".into(), 1.0));
        send(update::Event::Done(Ok("new".into())));
        app.handle_messages();
        assert_eq!(app.relaunch.as_deref(), Some(Path::new("new")));
        assert!(app.updating());
    }

    /// A job started now would be cut off by the restart.
    #[test]
    fn no_new_jobs_during_an_update() {
        let (mut app, _tx) = test_app();
        app.tools = Tool::ALL.iter().map(|&t| (t, ToolState::Ready { path: "bin/tool".into(), version: "1".into() })).collect();
        app.enqueue("https://example.com/video");
        app.update = Some(AppUpdate::Installing("9.9.9".into(), 0.0));
        app.schedule(&egui::Context::default());
        assert!(matches!(app.jobs[0].status, Status::Queued));
    }

    fn test_app() -> (App, Sender<Msg>) {
        let (tx, rx) = channel();
        let app = App {
            settings: Settings::default(),
            input: String::new(),
            tools: Vec::new(),
            jobs: Vec::new(),
            next_id: 0,
            log: Vec::new(),
            show_log: false,
            show_browser: false,
            cookie_check: CookieCheck::Idle,
            dirs: paths::AppDirs { bin: "bin".into(), config: "config".into(), portable: false },
            checking: false,
            tx: tx.clone(),
            rx,
            search: None,
            last_search: 0,
            live_search: Arc::default(),
            ytm: Arc::new(Mutex::new(ytm::Client::new())),
            preview: None,
            last_preview: 0,
            no_preview: HashMap::new(),
            update: None,
            relaunch: None,
            release_page: None,
        };
        (app, tx)
    }

    fn hit(url: &str) -> Hit {
        Hit {
            kind: Kind::Album,
            title: "Title".into(),
            subtitle: String::new(),
            artist: String::new(),
            seconds: None,
            explicit: false,
            thumb: None,
            url: url.into(),
        }
    }

    fn page(url: &str) -> search::Event {
        search::Event::Page(Kind::Song, Ok(ytm::SearchPage { hits: vec![hit(url)], next: None }))
    }

    #[test]
    fn search_results_are_library_audio() {
        let (mut app, _) = test_app();
        app.settings.mode = Mode::Video;
        app.settings.audio_format = AudioFormat::Mp3;
        app.add_hit(&hit("https://music.youtube.com/browse/MPREb_1"));
        let j = &app.jobs[0];
        assert_eq!(j.mode, Mode::Audio);
        assert_eq!(j.audio_format, AudioFormat::Mp3);
        assert!(library::handles(&j.url, j.mode));
        assert_eq!(j.name(), "Title");
    }

    #[test]
    fn late_search_results_are_dropped() {
        let (mut app, tx) = test_app();
        app.search = Some(Search::new(2, "new".into(), Kind::Song));
        tx.send(Msg::Search(1, page("old"))).unwrap();
        tx.send(Msg::Search(2, page("new"))).unwrap();
        app.handle_messages();
        let hits = &app.search.as_ref().unwrap().results[&Kind::Song].hits;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "new");
        app.close_search();
        tx.send(Msg::Search(2, page("closed"))).unwrap();
        app.handle_messages();
        assert!(app.search.is_none());
    }

    fn detail(hits: Vec<Hit>) -> ytm::Detail {
        ytm::Detail {
            summary: "2 songs · 8 minutes".into(),
            sections: vec![ytm::Section { title: String::new(), hits, more: None }],
            top: None,
        }
    }

    #[test]
    fn going_back_keeps_what_was_open() {
        let (mut app, tx) = test_app();
        let ctx = egui::Context::default();
        app.search = Some(Search::new(1, "q".into(), Kind::Album));
        app.search.as_mut().unwrap().add_page(Kind::Album, Ok(ytm::SearchPage { hits: vec![hit("album")], next: None }));
        app.open_hit(hit("album"), &ctx);
        tx.send(Msg::Search(1, search::Event::Detail("album".into(), Ok(detail(vec![hit("song")]))))).unwrap();
        app.handle_messages();
        app.open_hit(hit("artist"), &ctx);
        let s = app.search.as_mut().unwrap();
        assert_eq!(s.stack.len(), 2);
        s.stack.pop();
        s.stack.pop();
        // Results and the page read before are still there: nothing loads again.
        assert_eq!(s.results[&Kind::Album].hits.len(), 1);
        assert!(s.detail("album").is_some());
    }

    #[test]
    fn opening_a_read_page_is_instant() {
        let (mut app, _tx) = test_app();
        let ctx = egui::Context::default();
        let mut search = Search::new(1, "q".into(), Kind::Album);
        search.details.insert("album".into(), Loaded::Ready(detail(Vec::new())));
        app.search = Some(search);
        app.open_hit(hit("album"), &ctx);
        assert!(app.search.as_ref().unwrap().detail("album").is_some());
    }

    #[test]
    fn failed_pages_can_be_opened_again() {
        let (mut app, tx) = test_app();
        app.search = Some(Search::new(1, "q".into(), Kind::Album));
        tx.send(Msg::Search(1, search::Event::Detail("album".into(), Err("offline".into())))).unwrap();
        app.handle_messages();
        assert!(matches!(app.search.as_ref().unwrap().details["album"], Loaded::Failed(_)));
        app.open_hit(hit("album"), &egui::Context::default());
        assert!(matches!(app.search.as_ref().unwrap().details["album"], Loaded::Loading));
    }

    #[test]
    fn a_failed_preview_says_why_and_a_stopped_one_is_ignored() {
        let (mut app, tx) = test_app();
        app.preview = Some(preview::Preview::start(2, Hit { url: "x".into(), ..hit("x") }, None, |_| {}));
        tx.send(Msg::Preview(1, preview::Event::Ended(Err("old".into())))).unwrap();
        app.handle_messages();
        assert!(app.preview.is_some() && app.no_preview.is_empty());
        tx.send(Msg::Preview(2, preview::Event::Ended(Err("no clip".into())))).unwrap();
        app.handle_messages();
        assert!(app.preview.is_none());
        assert_eq!(app.no_preview["x"], "no clip");
    }

    #[test]
    fn artist_releases_keep_settings() {
        let (mut app, tx) = test_app();
        app.push_job("https://music.youtube.com/channel/UC1".into(), Mode::Audio, AudioFormat::Mp3);
        let links = vec!["https://music.youtube.com/browse/MPREb_1".into(), "https://music.youtube.com/browse/MPREb_2".into()];
        tx.send(Msg::Job(0, Event::Enqueue(links))).unwrap();
        app.handle_messages();
        assert_eq!(app.jobs.len(), 3);
        assert_eq!(app.jobs.iter().find(|j| j.id == 0).unwrap().queued, 2);
        // Newest first: the first release is the oldest new job, so it runs first.
        let releases: Vec<_> = app.jobs.iter().rev().skip(1).collect();
        assert!(releases[0].url.ends_with("MPREb_1") && releases[1].url.ends_with("MPREb_2"));
        assert!(releases.iter().all(|j| j.mode == Mode::Audio && j.audio_format == AudioFormat::Mp3));
        assert!(releases.iter().all(|j| matches!(j.status, Status::Queued)));
    }

    #[test]
    fn album_progress_and_summary() {
        let (mut app, tx) = test_app();
        app.push_job("https://music.youtube.com/browse/MPREb_1".into(), Mode::Auto, AudioFormat::M4a);
        let send = |e| tx.send(Msg::Job(0, e)).unwrap();
        let progress = |title: &str| Event::Progress { frac: Some(0.5), item: Some((2, 3)), title: title.into() };
        send(Event::Collection("Downface – Within".into()));
        send(Event::Skipped("01 - Lowmen.m4a".into()));
        send(progress("Cold and Overcome"));
        app.handle_messages();
        let j = &app.jobs[0];
        // Track titles don't replace the album name.
        assert_eq!(j.name(), "Downface – Within · Cold and Overcome");
        send(Event::Done("02 - Cold and Overcome.m4a".into()));
        send(progress("Purge"));
        send(Event::Done("03 - Purge.m4a".into()));
        tx.send(Msg::JobEnd(0, Ok(()))).unwrap();
        app.handle_messages();
        let j = &app.jobs[0];
        assert!(matches!(j.status, Status::Done));
        assert_eq!(j.done_label(), "✔ Downface – Within — 3 tracks (1 skipped)");
    }

    #[test]
    fn done_labels() {
        let (mut app, _) = test_app();
        app.push_job("https://x.com/a".into(), Mode::Auto, AudioFormat::M4a);
        let j = &mut app.jobs[0];
        assert_eq!(j.done_label(), "✔ https://x.com/a");
        j.title = "Get Lucky".into();
        j.files = vec!["a".into()];
        assert_eq!(j.done_label(), "✔ Get Lucky");
        j.skipped = 1;
        assert_eq!(j.done_label(), "✔ Get Lucky (already downloaded)");
        j.skipped = 0;
        j.files = vec!["a".into(), "b".into()];
        assert_eq!(j.done_label(), "✔ Get Lucky (2 files)");
        j.collection = "Downface alone".into();
        assert_eq!(j.done_label(), "✔ Downface alone — 2 tracks");
        j.skipped = 2;
        assert_eq!(j.done_label(), "✔ Downface alone — 2 tracks (all skipped)");
        j.files = vec!["a".into()];
        j.skipped = 0;
        assert_eq!(j.done_label(), "✔ Downface alone — 1 track");
        j.queued = 6;
        j.title = "Glassjaw".into();
        assert_eq!(j.done_label(), "✔ Glassjaw — 6 releases queued");
    }
}
