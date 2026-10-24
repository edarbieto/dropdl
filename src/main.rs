#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod job;
mod library;
mod lyrics;
mod music;
mod musicbrainz;
mod paths;
mod tags;
mod tools;
mod ytm;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use serde::{Deserialize, Serialize};

use job::{AudioFormat, BROWSER, Event, Mode, Request};
use tools::{Tool, ToolState};

const LOG_LIMIT: usize = 2000;
const PASTE_KEY: &str = if cfg!(target_os = "macos") { "⌘V" } else { "Ctrl+V" };

fn main() -> eframe::Result {
    let dirs = paths::resolve();
    let options = eframe::NativeOptions {
        persistence_path: Some(dirs.config.join("app.ron")),
        viewport: egui::ViewportBuilder::default()
            .with_title("dropdl")
            .with_inner_size([560.0, 420.0])
            .with_min_inner_size([420.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native("dropdl", options, Box::new(|cc| Ok(Box::new(App::new(cc, dirs)))))
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
            (true, true) => self.url.clone(),
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
        };
        let kind = if app.dirs.portable { "portable" } else { "per-user" };
        let base = app.dirs.bin.parent().unwrap_or(&app.dirs.bin).display().to_string();
        app.log(format!("data folder ({kind}): {base}"));
        app.check_tools(&cc.egui_ctx);
        app
    }

    /// Validates, installs and updates the tools in the background.
    fn check_tools(&mut self, ctx: &egui::Context) {
        self.checking = true;
        let (tx, ctx, bin) = (self.tx.clone(), ctx.clone(), self.dirs.bin.clone());
        std::thread::spawn(move || {
            let send = |msg| {
                let _ = tx.send(msg);
                ctx.request_repaint();
            };
            tools::ensure_all(&bin, |tool, state| send(Msg::Tool(tool, state)), |line| send(Msg::Log(line)));
            send(Msg::ToolsChecked);
        });
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

    fn log(&mut self, line: String) {
        self.log.push(line);
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
            }
        }
    }

    /// Starts the next queued job if nothing is running.
    fn schedule(&mut self, ctx: &egui::Context) {
        let busy = self
            .jobs
            .iter()
            .any(|j| matches!(j.status, Status::Running | Status::Processing(_)));
        if busy {
            return;
        }
        let Some(tools) = self.job_tools() else { return };
        // Oldest first: jobs are stored newest first.
        let Some(j) = self.jobs.iter_mut().rev().find(|j| matches!(j.status, Status::Queued)) else {
            return;
        };
        j.status = Status::Running;
        let req = Request {
            url: j.url.clone(),
            mode: j.mode,
            audio_format: j.audio_format,
            out_dir: self.settings.out_dir.clone(),
            browser: self.settings.browser.clone(),
            template: None,
        };
        let (id, pid, cancel, tx, ctx) = (j.id, j.pid.clone(), j.cancel.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let send = |event| {
                let _ = tx.send(Msg::Job(id, event));
                ctx.request_repaint();
            };
            let result = if library::handles(&req.url, req.mode) {
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
                let busy = self.jobs.iter().any(|j| matches!(j.status, Status::Running | Status::Processing(_)));
                if ui
                    .add_enabled(!self.checking && !busy, egui::Button::new("Check updates").small())
                    .on_hover_text("Re-check tools and update them")
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
                .hint_text(format!("Paste a link ({PASTE_KEY})"))
                .desired_width(ui.available_width() - button_w - ui.spacing().item_spacing.x);
            let resp = ui.add(edit);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let valid = job::parse_url(&self.input).is_some();
            let clicked = ui
                .add_enabled(valid, egui::Button::new("Download").min_size([button_w, 0.0].into()))
                .clicked();
            if (clicked || enter) && valid {
                let text = std::mem::take(&mut self.input);
                self.enqueue(&text);
            }
        });
        if !ready {
            ui.label(egui::RichText::new("Preparing tools… downloads will start when ready.").weak().small());
        }

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
                .link(self.settings.out_dir.display().to_string())
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
            ui.separator();

            let log_h = if self.show_log { 140.0 } else { 0.0 };
            egui::ScrollArea::vertical()
                .id_salt("jobs")
                .max_height((ui.available_height() - log_h - 28.0).max(60.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
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

    /// An app without a window, and a sender for its messages.
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
        };
        (app, tx)
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
