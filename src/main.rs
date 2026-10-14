#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod job;
mod tools;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use eframe::egui;
use serde::{Deserialize, Serialize};

use job::{BROWSERS, Event, Mode, Request};
use tools::{Tool, ToolState};

const LOG_LIMIT: usize = 2000;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("dropdl")
            .with_inner_size([560.0, 420.0])
            .with_min_inner_size([420.0, 300.0]),
        ..Default::default()
    };
    eframe::run_native("dropdl", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    out_dir: PathBuf,
    mode: Mode,
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
            browser: None,
        }
    }
}

enum Msg {
    Tool(Tool, ToolState),
    Job(usize, Event),
    JobEnd(usize, Result<(), String>),
    Cookies(String, Result<usize, String>),
    Updated(Result<String, String>),
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
    status: Status,
    title: String,
    frac: Option<f32>,
    item: Option<(u32, u32)>,
    files: Vec<PathBuf>,
    pid: Arc<Mutex<Option<u32>>>,
    cancelled: bool,
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
    updating: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        add_fallback_fonts(&cc.egui_ctx);
        let (tx, rx) = channel();
        let app = Self {
            settings,
            input: String::new(),
            tools: Tool::ALL.iter().map(|&t| (t, ToolState::Checking)).collect(),
            jobs: Vec::new(),
            next_id: 0,
            log: Vec::new(),
            show_log: false,
            show_browser: false,
            cookie_check: CookieCheck::Idle,
            updating: false,
            tx,
            rx,
        };
        app.check_tools(&cc.egui_ctx);
        app
    }

    fn check_tools(&self, ctx: &egui::Context) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            tools::ensure_all(|tool, state| {
                let _ = tx.send(Msg::Tool(tool, state));
                ctx.request_repaint();
            });
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
        self.jobs.insert(
            0,
            Job {
                id: self.next_id,
                url,
                mode: self.settings.mode,
                status: Status::Queued,
                title: String::new(),
                frac: None,
                item: None,
                files: Vec::new(),
                pid: Arc::default(),
                cancelled: false,
            },
        );
        self.next_id += 1;
        true
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
                        Event::Log(line) => self.log(line),
                    }
                }
                Msg::JobEnd(id, result) => {
                    let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) else { continue };
                    j.status = match result {
                        _ if j.cancelled => Status::Cancelled,
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
                Msg::Updated(result) => {
                    self.updating = false;
                    let line = match result {
                        Ok(s) => format!("yt-dlp update: {s}"),
                        Err(e) => format!("yt-dlp update failed: {e}"),
                    };
                    self.log(line);
                    // Re-read the version.
                    self.tools[0].1 = ToolState::Checking;
                    let tx = self.tx.clone();
                    std::thread::spawn(move || {
                        let state = match tools::find(Tool::YtDlp) {
                            Some((path, version)) => ToolState::Ready { path, version },
                            None => ToolState::Failed("not found".into()),
                        };
                        let _ = tx.send(Msg::Tool(Tool::YtDlp, state));
                    });
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
            out_dir: self.settings.out_dir.clone(),
            browser: self.settings.browser.clone(),
        };
        let (id, pid, tx, ctx) = (j.id, j.pid.clone(), self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = job::run(&req, &tools, &pid, |event| {
                let _ = tx.send(Msg::Job(id, event));
                ctx.request_repaint();
            });
            let _ = tx.send(Msg::JobEnd(id, result));
            ctx.request_repaint();
        });
    }

    fn test_browser(&mut self, ctx: &egui::Context, browser: &str) {
        let Some(ytdlp) = self.tool_path(Tool::YtDlp) else { return };
        self.cookie_check = CookieCheck::Testing;
        let (tx, ctx, browser) = (self.tx.clone(), ctx.clone(), browser.to_string());
        std::thread::spawn(move || {
            let result = job::test_cookies(&ytdlp, &browser);
            let _ = tx.send(Msg::Cookies(browser, result));
            ctx.request_repaint();
        });
    }

    fn update_ytdlp(&mut self, ctx: &egui::Context) {
        let Some(ytdlp) = self.tool_path(Tool::YtDlp) else { return };
        self.updating = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Updated(tools::update_ytdlp(&ytdlp)));
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
                let failed = self.tools.iter().any(|(_, s)| matches!(s, ToolState::Failed(_)));
                if failed && ui.small_button("Retry").clicked() {
                    self.check_tools(ui.ctx());
                }
                let can_update = !self.updating && self.tool_path(Tool::YtDlp).is_some();
                if ui
                    .add_enabled(can_update, egui::Button::new("Update yt-dlp").small())
                    .clicked()
                {
                    self.update_ytdlp(ui.ctx());
                }
                let label = match &self.settings.browser {
                    Some(b) => format!("🍪 {}", browser_label(b)),
                    None => "Connect browser".to_string(),
                };
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
            ui.label("Use your browser's login for sites that need it (Instagram, Facebook, private videos).");
            ui.label(
                egui::RichText::new("Firefox works best. Chromium browsers may need to be closed first.")
                    .weak()
                    .small(),
            );
            ui.horizontal_wrapped(|ui| {
                let testing = matches!(self.cookie_check, CookieCheck::Testing);
                for (label, name) in BROWSERS {
                    let selected = self.settings.browser.as_deref() == Some(name);
                    if ui
                        .add_enabled(!testing, egui::Button::selectable(selected, label))
                        .clicked()
                    {
                        self.test_browser(ui.ctx(), name);
                    }
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
                }
            }
        });
    }

    fn input_row(&mut self, ui: &mut egui::Ui) {
        let ready = self.job_tools().is_some();
        ui.horizontal(|ui| {
            let button_w = 90.0;
            let edit = egui::TextEdit::singleline(&mut self.input)
                .hint_text("Paste a link (Ctrl+V)")
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
                .on_hover_text("YouTube Music → MP3, everything else → MP4");
            ui.radio_value(&mut self.settings.mode, Mode::Video, "Video (MP4)");
            ui.radio_value(&mut self.settings.mode, Mode::Audio, "Audio (MP3)");
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
                let name = if j.title.is_empty() { j.url.as_str() } else { j.title.as_str() };
                let kind = if job::wants_audio(&j.url, j.mode) { "MP3" } else { "MP4" };
                let item = j.item.map(|(i, n)| format!(" [{i}/{n}]")).unwrap_or_default();

                let active = matches!(j.status, Status::Running | Status::Processing(_));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if active {
                        if ui.small_button("✖").on_hover_text("Cancel").clicked() {
                            j.cancelled = true;
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
                                let text = match j.files.len() {
                                    0 | 1 => format!("✔ {name}"),
                                    n => format!("✔ {name} ({n} files)"),
                                };
                                if ui.link(text).on_hover_text("Show in folder").clicked() {
                                    match j.files.first() {
                                        Some(f) => reveal(f),
                                        None => open_path(&out_dir),
                                    }
                                }
                            }
                            Status::Failed(e) => {
                                ui.colored_label(ui.visuals().error_fg_color, format!("✖ {name}"))
                                    .on_hover_text(e);
                            }
                            Status::Cancelled => {
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

        // Ctrl+V anywhere outside the text box downloads the clipboard link right away.
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
                            ui.label(egui::RichText::new("Copy a link and press Ctrl+V here.").weak());
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

/// egui's built-in font has no CJK/Korean glyphs; borrow them from Windows so
/// titles in those scripts don't render as boxes.
fn add_fallback_fonts(ctx: &egui::Context) {
    let fonts_dir = PathBuf::from(std::env::var_os("WINDIR").unwrap_or("C:\\Windows".into())).join("Fonts");
    let mut defs = egui::FontDefinitions::default();
    for file in ["msyh.ttc", "malgun.ttf"] {
        let Ok(bytes) = std::fs::read(fonts_dir.join(file)) else { continue };
        defs.font_data.insert(file.into(), Arc::new(egui::FontData::from_owned(bytes)));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(file.into());
        }
    }
    ctx.set_fonts(defs);
}

fn browser_label(name: &str) -> &str {
    BROWSERS.iter().find(|(_, n)| *n == name).map_or(name, |(l, _)| l)
}

fn open_path(path: &std::path::Path) {
    let _ = tools::command("explorer").arg(path).spawn();
}

/// Opens Explorer with `file` selected.
fn reveal(file: &std::path::Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = tools::command("explorer")
            .raw_arg(format!("/select,\"{}\"", file.display()))
            .spawn();
    }
    #[cfg(not(windows))]
    open_path(file.parent().unwrap_or(file));
}
