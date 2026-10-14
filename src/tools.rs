//! Detection, validation and automatic download of the external binaries
//! dropdl depends on: yt-dlp, deno (JS runtime for YouTube) and ffmpeg/ffprobe.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
const DENO_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
const FFMPEG_URL: &str = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    YtDlp,
    Deno,
    Ffmpeg,
    Ffprobe,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::YtDlp, Tool::Deno, Tool::Ffmpeg, Tool::Ffprobe];

    pub fn name(self) -> &'static str {
        match self {
            Tool::YtDlp => "yt-dlp",
            Tool::Deno => "deno",
            Tool::Ffmpeg => "ffmpeg",
            Tool::Ffprobe => "ffprobe",
        }
    }

    fn exe(self) -> String {
        format!("{}.exe", self.name())
    }

    fn version_arg(self) -> &'static str {
        match self {
            Tool::YtDlp | Tool::Deno => "--version",
            Tool::Ffmpeg | Tool::Ffprobe => "-version",
        }
    }
}

#[derive(Clone, Debug)]
pub enum ToolState {
    Checking,
    Downloading(f32),
    Ready { path: PathBuf, version: String },
    Failed(String),
}

impl ToolState {
    pub fn path(&self) -> Option<&Path> {
        match self {
            ToolState::Ready { path, .. } => Some(path),
            _ => None,
        }
    }
}

/// Where dropdl keeps its own copies of the tools: `%LOCALAPPDATA%\dropdl\bin`.
pub fn bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dropdl")
        .join("bin")
}

/// A `Command` that never flashes a console window on Windows.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Checks every tool and downloads the missing ones, reporting each state change.
pub fn ensure_all(report: impl Fn(Tool, ToolState)) {
    for tool in Tool::ALL {
        report(tool, ToolState::Checking);
    }
    let mut found: Vec<_> = Tool::ALL.iter().map(|&t| find(t)).collect();
    // yt-dlp is given one directory for both ffmpeg and ffprobe, so a mixed
    // pair (e.g. one on PATH, one elsewhere) counts as missing.
    let ff_dir = |i: usize| found[i].as_ref().and_then(|(p, _)| p.parent().map(Path::to_path_buf));
    if ff_dir(2) != ff_dir(3) {
        found[2] = None;
        found[3] = None;
    }
    let mut missing = Vec::new();
    for (tool, hit) in Tool::ALL.into_iter().zip(found) {
        match hit {
            Some((path, version)) => report(tool, ToolState::Ready { path, version }),
            None => missing.push(tool),
        }
    }

    if missing.contains(&Tool::YtDlp) {
        install(&[Tool::YtDlp], &report, |dest, p| download(YTDLP_URL, &dest.join("yt-dlp.exe"), p));
    }
    if missing.contains(&Tool::Deno) {
        install(&[Tool::Deno], &report, |dest, p| download_zip(DENO_URL, dest, &["deno.exe"], p));
    }
    // ffmpeg and ffprobe ship in the same archive.
    let ff: Vec<Tool> = missing
        .iter()
        .copied()
        .filter(|t| matches!(t, Tool::Ffmpeg | Tool::Ffprobe))
        .collect();
    if !ff.is_empty() {
        install(&ff, &report, |dest, p| {
            download_zip(FFMPEG_URL, dest, &["ffmpeg.exe", "ffprobe.exe"], p)
        });
    }
}

/// Looks for a working copy of `tool`, first in our bin dir, then on PATH.
pub fn find(tool: Tool) -> Option<(PathBuf, String)> {
    let exe = tool.exe();
    let mut candidates = vec![bin_dir().join(&exe)];
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|dir| dir.join(&exe)));
    }
    candidates
        .into_iter()
        .filter(|p| p.is_file())
        .find_map(|p| version(tool, &p).map(|v| (p, v)))
}

fn version(tool: Tool, path: &Path) -> Option<String> {
    let out = command(path).arg(tool.version_arg()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let first = text.lines().next()?.trim();
    // "2026.08.19" / "deno 2.5.0 (stable, ...)" / "ffmpeg version N-1234-g... Copyright ..."
    let v = match tool {
        Tool::YtDlp => first,
        Tool::Deno => first.split_whitespace().nth(1).unwrap_or(first),
        Tool::Ffmpeg | Tool::Ffprobe => first.split_whitespace().nth(2).unwrap_or(first),
    };
    Some(v.to_string())
}

fn install(
    tools: &[Tool],
    report: &impl Fn(Tool, ToolState),
    fetch: impl FnOnce(&Path, &mut dyn FnMut(f32)) -> Result<(), String>,
) {
    let dest = bin_dir();
    let result = fs::create_dir_all(&dest)
        .map_err(|e| e.to_string())
        .and_then(|_| {
            fetch(&dest, &mut |p| {
                for &t in tools {
                    report(t, ToolState::Downloading(p));
                }
            })
        });
    for &tool in tools {
        let state = match &result {
            Err(e) => ToolState::Failed(e.clone()),
            Ok(()) => match find(tool) {
                Some((path, version)) => ToolState::Ready { path, version },
                None => ToolState::Failed("downloaded but does not run".into()),
            },
        };
        report(tool, state);
    }
}

/// Downloads `url` to `dest` atomically (via a `.part` file).
fn download(url: &str, dest: &Path, progress: &mut dyn FnMut(f32)) -> Result<(), String> {
    let part = dest.with_extension("part");
    let result = (|| {
        let mut resp = ureq::get(url).call().map_err(|e| format!("{url}: {e}"))?;
        let total: u64 = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let mut reader = resp.body_mut().as_reader();
        let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 64 * 1024];
        let mut done: u64 = 0;
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if total > 0 {
                progress(done as f32 / total as f32);
            }
        }
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&part, dest).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&part);
    }
    result
}

/// Downloads a zip and extracts only the files named in `wanted` (matched by
/// file name, anywhere in the archive) flat into `dest`.
fn download_zip(
    url: &str,
    dest: &Path,
    wanted: &[&str],
    progress: &mut dyn FnMut(f32),
) -> Result<(), String> {
    let zip_path = dest.join("download.zip");
    download(url, &zip_path, progress)?;
    let result = (|| {
        let file = fs::File::open(&zip_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        let mut found = 0;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let full = entry.name().map_err(|e| e.to_string())?.to_string();
            let name = full.rsplit('/').next().unwrap_or_default().to_string();
            if !wanted.contains(&name.as_str()) {
                continue;
            }
            let mut out = fs::File::create(dest.join(&name)).map_err(|e| e.to_string())?;
            io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            found += 1;
        }
        if found < wanted.len() {
            return Err(format!("archive is missing {}", wanted.join(", ")));
        }
        Ok(())
    })();
    let _ = fs::remove_file(&zip_path);
    result
}

/// Runs `yt-dlp -U` and returns its output.
pub fn update_ytdlp(ytdlp: &Path) -> Result<String, String> {
    let out = command(ytdlp).arg("-U").output().map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let last = text.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").to_string();
    if out.status.success() { Ok(last) } else { Err(last) }
}
