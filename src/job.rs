//! Turning a link into yt-dlp arguments, running yt-dlp and parsing its output.
//! The format rules are documented in docs/download-rules.md.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::tools::command;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Auto,
    Video,
    Audio,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum AudioFormat {
    /// Copies the source's AAC stream when there is one, otherwise converts.
    #[default]
    M4a,
    Mp3,
}

impl AudioFormat {
    pub fn label(self) -> &'static str {
        match self {
            AudioFormat::M4a => "M4A",
            AudioFormat::Mp3 => "MP3",
        }
    }
}

/// Browsers yt-dlp can read cookies from (label, yt-dlp name).
pub const BROWSERS: [(&str, &str); 6] = [
    ("Firefox", "firefox"),
    ("Chrome", "chrome"),
    ("Edge", "edge"),
    ("Brave", "brave"),
    ("Opera", "opera"),
    ("Vivaldi", "vivaldi"),
];

/// Paths to the binaries a download needs.
#[derive(Clone)]
pub struct Tools {
    pub ytdlp: PathBuf,
    pub deno: PathBuf,
    pub ffmpeg: PathBuf,
}

pub struct Request {
    pub url: String,
    pub mode: Mode,
    pub audio_format: AudioFormat,
    pub out_dir: PathBuf,
    pub browser: Option<String>,
}

/// Something worth showing that yt-dlp reported.
pub enum Event {
    Progress {
        frac: Option<f32>,
        item: Option<(u32, u32)>,
        title: String,
    },
    Processing(String),
    Done(PathBuf),
    Log(String),
}

/// Returns a normalized URL if `text` looks like a single link.
pub fn parse_url(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return None;
    }
    if t.starts_with("http://") || t.starts_with("https://") {
        return Some(t.to_string());
    }
    // Accept "youtu.be/abc" style links without a scheme.
    let host = t.split('/').next()?;
    (host.contains('.') && !host.starts_with('.')).then(|| format!("https://{t}"))
}

fn host(url: &str) -> &str {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    host.strip_prefix("www.").unwrap_or(host)
}

fn is_youtube_music(url: &str) -> bool {
    host(url) == "music.youtube.com"
}

/// Whether the link points at a whole collection (playlist, album) rather
/// than a single item that merely sits inside one.
fn is_collection(url: &str) -> bool {
    let has_video = url.contains("v=") || url.contains("youtu.be/") || url.contains("/shorts/");
    url.contains("/playlist") || url.contains("/browse/") || (url.contains("list=") && !has_video)
}

/// Sites that only serve audio; Auto mode downloads them as audio.
const AUDIO_HOSTS: [&str; 5] = [
    "music.youtube.com",
    "soundcloud.com",
    "bandcamp.com",
    "mixcloud.com",
    "audiomack.com",
];

/// Whether this request ends up as audio only.
pub fn wants_audio(url: &str, mode: Mode) -> bool {
    match mode {
        Mode::Audio => true,
        Mode::Video => false,
        Mode::Auto => {
            let h = host(url);
            AUDIO_HOSTS.iter().any(|a| h == *a || h.ends_with(&format!(".{a}")))
        }
    }
}

pub fn build_args(req: &Request, tools: &Tools) -> Vec<String> {
    let audio = wants_audio(&req.url, req.mode);
    let music = is_youtube_music(&req.url);
    let collection = is_collection(&req.url);

    let mut a: Vec<String> = Vec::new();
    let mut push = |items: &[&str]| a.extend(items.iter().map(|s| s.to_string()));

    push(&["--newline", "--progress", "--no-simulate", "--no-mtime", "--no-playlist", "--encoding", "utf-8"]);
    push(&[
        "--progress-template",
        "download:[dl]%(progress.downloaded_bytes)s|%(progress.total_bytes,progress.total_bytes_estimate)s|%(info.playlist_index)s|%(info.n_entries)s|%(info.title)s",
        "--progress-template",
        "postprocess:[pp]%(progress.postprocessor)s",
        "--print",
        "after_move:[done]%(filepath)s",
    ]);
    push(&["--ffmpeg-location", &tools.ffmpeg.to_string_lossy()]);
    push(&["--js-runtimes", &format!("deno:{}", tools.deno.display())]);
    push(&["-P", &req.out_dir.to_string_lossy()]);
    if let Some(browser) = &req.browser {
        push(&["--cookies-from-browser", browser]);
    }

    if audio {
        match req.audio_format {
            // m4a keeps an AAC source as is (YouTube's 140/141); anything else is converted.
            AudioFormat::M4a => push(&["-f", "ba[acodec^=mp4a]/ba/b", "-x", "--audio-format", "m4a"]),
            AudioFormat::Mp3 => push(&["-t", "mp3", "--audio-quality", "0"]),
        }
        push(&["--embed-metadata", "--embed-thumbnail"]);
        if music {
            // YouTube Music covers are 16:9 frames with the art in the middle: crop to square.
            push(&[
                "--ppa",
                r#"ThumbnailsConvertor+FFmpeg_o:-c:v mjpeg -vf crop="'if(gt(ih,iw),iw,ih)':'if(gt(iw,ih),ih,iw)'""#,
            ]);
        }
        let template = match (music, collection) {
            (_, true) => {
                push(&["--parse-metadata", "playlist_index:%(track_number)s"]);
                "%(album,playlist_title)s/%(playlist_index)02d - %(title)s.%(ext)s"
            }
            (true, false) => "%(artist,creator,uploader|Unknown)s - %(title)s.%(ext)s",
            (false, false) => "%(title).150B.%(ext)s",
        };
        push(&["-o", template]);
    } else {
        push(&["-t", "mp4", "--embed-metadata"]);
        let template = if collection {
            "%(playlist_title)s/%(playlist_index)03d - %(title).120B.%(ext)s"
        } else {
            "%(title).120B [%(id)s].%(ext)s"
        };
        push(&["-o", template]);
    }

    a.push("--".into());
    a.push(req.url.clone());
    a
}

fn parse_line(line: &str) -> Event {
    if let Some(rest) = line.strip_prefix("[dl]") {
        let mut f = rest.splitn(5, '|');
        let mut num = || f.next().and_then(|s| s.trim().parse::<f64>().ok());
        let (done, total, index, count) = (num(), num(), num(), num());
        let title = f.next().unwrap_or("").trim().to_string();
        let frac = match (done, total) {
            (Some(d), Some(t)) if t > 0.0 => Some((d / t).clamp(0.0, 1.0) as f32),
            _ => None,
        };
        let item = match (index, count) {
            (Some(i), Some(n)) => Some((i as u32, n as u32)),
            _ => None,
        };
        return Event::Progress { frac, item, title };
    }
    if let Some(pp) = line.strip_prefix("[pp]") {
        return Event::Processing(pp.trim().to_string());
    }
    if let Some(path) = line.strip_prefix("[done]") {
        return Event::Done(PathBuf::from(path.trim()));
    }
    Event::Log(line.to_string())
}

/// Runs yt-dlp to completion, forwarding events. `pid` is filled while the
/// process runs so it can be cancelled with [`kill`].
pub fn run(
    req: &Request,
    tools: &Tools,
    pid: &Arc<Mutex<Option<u32>>>,
    mut on_event: impl FnMut(Event),
) -> Result<(), String> {
    let args = build_args(req, tools);
    on_event(Event::Log(format!("> yt-dlp {}", args.join(" "))));

    let mut child = command(&tools.ytdlp)
        .args(&args)
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start yt-dlp: {e}"))?;
    *pid.lock().unwrap() = Some(child.id());

    let stderr = child.stderr.take().unwrap();
    let err_thread = std::thread::spawn(move || {
        BufReader::new(stderr)
            .split(b'\n')
            .map_while(Result::ok)
            .map(|l| String::from_utf8_lossy(&l).trim_end().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
    });

    for line in BufReader::new(child.stdout.take().unwrap()).split(b'\n').map_while(Result::ok) {
        let line = String::from_utf8_lossy(&line).trim_end().to_string();
        if !line.is_empty() {
            on_event(parse_line(&line));
        }
    }

    let status = child.wait().map_err(|e| e.to_string())?;
    *pid.lock().unwrap() = None;
    let errors = err_thread.join().unwrap_or_default();
    for line in &errors {
        on_event(Event::Log(line.clone()));
    }

    if status.success() {
        Ok(())
    } else {
        Err(last_error(&errors).unwrap_or_else(|| format!("yt-dlp exited with {status}")))
    }
}

fn last_error(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .rev()
        .find_map(|l| l.strip_prefix("ERROR:"))
        .map(|e| e.trim().to_string())
}

/// Kills a running yt-dlp and its children (the .exe is a launcher that
/// spawns the real Python process).
pub fn kill(pid: u32) {
    let _ = command("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

/// Checks that yt-dlp can read cookies from `browser`; returns how many it found.
/// `--list-impersonate-targets` makes yt-dlp load cookies without touching the network.
pub fn test_cookies(ytdlp: &Path, browser: &str) -> Result<usize, String> {
    let out = command(ytdlp)
        .args(["--cookies-from-browser", browser, "--list-impersonate-targets"])
        .env("PYTHONUTF8", "1")
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("Extracted ") {
            if let Some(n) = rest.split_whitespace().next().and_then(|n| n.parse().ok()) {
                return Ok(n);
            }
        }
    }
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    Err(last_error(&lines).unwrap_or_else(|| "could not read cookies".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(parse_url(" https://x.com/a "), Some("https://x.com/a".into()));
        assert_eq!(parse_url("youtu.be/abc"), Some("https://youtu.be/abc".into()));
        assert_eq!(parse_url("hello world"), None);
        assert_eq!(parse_url("hello"), None);
    }

    #[test]
    fn classification() {
        let song = "https://music.youtube.com/watch?v=abc&list=RDAMVMabc";
        let album = "https://music.youtube.com/playlist?list=OLAK5uy_xyz";
        let video = "https://www.youtube.com/watch?v=abc&list=PL123";
        assert!(wants_audio(song, Mode::Auto) && !is_collection(song));
        assert!(wants_audio(album, Mode::Auto) && is_collection(album));
        assert!(!wants_audio(video, Mode::Auto) && !is_collection(video));
        assert!(wants_audio(video, Mode::Audio));
        assert!(!wants_audio("https://www.tiktok.com/@u/video/1", Mode::Auto));
        assert!(wants_audio("https://soundcloud.com/a/b", Mode::Auto));
        assert!(wants_audio("https://artist.bandcamp.com/album/x", Mode::Auto));
    }

    #[test]
    fn audio_args() {
        let tools = Tools { ytdlp: "y".into(), deno: "d".into(), ffmpeg: "f".into() };
        let args = |audio_format| {
            let req = Request {
                url: "https://music.youtube.com/watch?v=abc".into(),
                mode: Mode::Auto,
                audio_format,
                out_dir: "o".into(),
                browser: None,
            };
            build_args(&req, &tools).join(" ")
        };
        let m4a = args(AudioFormat::M4a);
        assert!(m4a.contains("-f ba[acodec^=mp4a]/ba/b -x --audio-format m4a --embed-metadata --embed-thumbnail"));
        assert!(!m4a.contains("mp3"));
        let mp3 = args(AudioFormat::Mp3);
        assert!(mp3.contains("-t mp3 --audio-quality 0 --embed-metadata --embed-thumbnail"));
        assert!(!mp3.contains("m4a"));
    }

    #[test]
    fn progress_line() {
        match parse_line("[dl]500|1000|2|10|Some | title") {
            Event::Progress { frac, item, title } => {
                assert_eq!(frac, Some(0.5));
                assert_eq!(item, Some((2, 10)));
                assert_eq!(title, "Some | title");
            }
            _ => panic!(),
        }
        match parse_line("[dl]500|NA|NA|NA|t") {
            Event::Progress { frac, item, .. } => assert!(frac.is_none() && item.is_none()),
            _ => panic!(),
        }
    }
}

/// End-to-end check: installs missing tools and downloads a short public video
/// as MP4 and as M4A. Needs network. Run with:
/// `cargo test -- --ignored --nocapture` (set DROPDL_TEST_OUT to choose the folder).
#[cfg(test)]
mod e2e {
    use super::*;
    use crate::tools::{self, Tool, ToolState};

    #[test]
    #[ignore]
    fn install_and_download() {
        let found = std::sync::Mutex::new(std::collections::HashMap::new());
        let dirs = crate::paths::resolve();
        let report = |tool: Tool, state: ToolState| {
            if !matches!(state, ToolState::Downloading(_)) {
                println!("{} {state:?}", tool.name());
            }
            if let ToolState::Ready { path, .. } = state {
                found.lock().unwrap().insert(tool.name(), path);
            }
        };
        tools::ensure_all(&dirs.bin, report, |line| println!("{line}"));
        let found = found.into_inner().unwrap();
        assert_eq!(found.len(), Tool::ALL.len(), "some tools are missing");
        let tools = Tools {
            ytdlp: found["yt-dlp"].clone(),
            deno: found["deno"].clone(),
            ffmpeg: found["ffmpeg"].parent().unwrap().to_path_buf(),
        };
        let out_dir = std::env::var_os("DROPDL_TEST_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dropdl-test"));
        let urls = std::env::var("DROPDL_TEST_URLS").unwrap_or_else(|_| {
            "https://www.youtube.com/watch?v=jNQXAC9IVRw https://music.youtube.com/watch?v=jNQXAC9IVRw".into()
        });
        for url in urls.split_whitespace() {
            let req = Request {
                url: url.into(),
                mode: Mode::Auto,
                audio_format: AudioFormat::M4a,
                out_dir: out_dir.clone(),
                browser: None,
            };
            let mut files = Vec::new();
            let result = run(&req, &tools, &Arc::default(), |e| match e {
                Event::Done(p) => files.push(p),
                Event::Log(l) => println!("{l}"),
                _ => {}
            });
            assert!(result.is_ok(), "{url}: {result:?}");
            assert_eq!(files.len(), 1, "{url}");
            let ext = files[0].extension().unwrap().to_string_lossy().to_string();
            assert_eq!(ext, if wants_audio(url, Mode::Auto) { "m4a" } else { "mp4" });
            println!("OK {}", files[0].display());
        }
    }
}
