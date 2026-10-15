//! Detection, validation, download and update of the external binaries
//! dropdl depends on: yt-dlp, deno (JS runtime for YouTube) and ffmpeg/ffprobe.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const YTDLP_REPO: &str = "yt-dlp/yt-dlp";
const DENO_REPO: &str = "denoland/deno";
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
        format!("{}{}", self.name(), std::env::consts::EXE_SUFFIX)
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

/// Startup check of the `bin` folder: validates every tool, downloads the
/// missing ones and updates yt-dlp/deno when a newer release exists.
/// A tool on PATH is only used if dropdl can't get its own copy.
pub fn ensure_all(bin: &Path, report: impl Fn(Tool, ToolState), log: impl Fn(String)) {
    for tool in Tool::ALL {
        report(tool, ToolState::Checking);
    }
    let local: Vec<_> = Tool::ALL.iter().map(|&t| local(bin, t)).collect();
    for (&tool, hit) in Tool::ALL.iter().zip(&local) {
        if let Some((path, version)) = hit {
            report(tool, ToolState::Ready { path: path.clone(), version: version.clone() });
        }
    }

    for (tool, repo) in [(Tool::YtDlp, YTDLP_REPO), (Tool::Deno, DENO_REPO)] {
        let current = local[tool as usize].as_ref().map(|(_, v)| v.as_str());
        if let Some(v) = current {
            match latest_version(repo) {
                Some(latest) if latest != v => log(format!("{}: updating {v} → {latest}", tool.name())),
                Some(_) => continue,
                None => {
                    log(format!("{}: could not check for updates", tool.name()));
                    continue;
                }
            }
        }
        install(&[tool], bin, &report, &log, |p| match tool {
            Tool::YtDlp => download(YTDLP_URL, &bin.join(tool.exe()), p),
            _ => download_zip(DENO_URL, bin, &[&tool.exe()], p),
        });
    }

    // ffmpeg and ffprobe ship in the same archive and must sit side by side.
    if local[Tool::Ffmpeg as usize].is_none() || local[Tool::Ffprobe as usize].is_none() {
        let names = [Tool::Ffmpeg.exe(), Tool::Ffprobe.exe()];
        install(&[Tool::Ffmpeg, Tool::Ffprobe], bin, &report, &log, |p| {
            download_zip(FFMPEG_URL, bin, &[&names[0], &names[1]], p)
        });
    }
}

/// dropdl's own copy of `tool`, if it runs.
fn local(bin: &Path, tool: Tool) -> Option<(PathBuf, String)> {
    let path = bin.join(tool.exe());
    let v = version(tool, &path)?;
    Some((path, v))
}

/// The first working copy of `tool` on PATH.
fn on_path(tool: Tool) -> Option<(PathBuf, String)> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(tool.exe()))
        .filter(|p| p.is_file())
        .find_map(|p| version(tool, &p).map(|v| (p, v)))
}

fn version(tool: Tool, path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
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

/// Latest release tag of a GitHub repo (without a leading "v"), read from the
/// `/releases/latest` redirect so it doesn't count against the API rate limit.
fn latest_version(repo: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let resp = agent
        .get(&format!("https://github.com/{repo}/releases/latest"))
        .call()
        .ok()?;
    let location = resp.headers().get("location")?.to_str().ok()?;
    let tag = location.rsplit('/').next()?;
    Some(tag.trim_start_matches('v').to_string())
}

/// Downloads `tools` with `fetch`, then reports their final state. If the
/// download failed, an existing copy (old version, or one on PATH) is kept.
fn install(
    tools: &[Tool],
    bin: &Path,
    report: &impl Fn(Tool, ToolState),
    log: &impl Fn(String),
    fetch: impl FnOnce(&mut dyn FnMut(f32)) -> Result<(), String>,
) {
    let result = fetch(&mut |p| {
        for &t in tools {
            report(t, ToolState::Downloading(p));
        }
    });
    if let Err(e) = &result {
        log(format!("{}: download failed: {e}", tools[0].name()));
    }
    for &tool in tools {
        let state = match local(bin, tool).or_else(|| on_path(tool)) {
            Some((path, version)) => ToolState::Ready { path, version },
            None => ToolState::Failed(match &result {
                Err(e) => e.clone(),
                Ok(()) => "downloaded but does not run".into(),
            }),
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
            let target = dest.join(&name);
            let part = target.with_extension("part");
            let mut out = fs::File::create(&part).map_err(|e| e.to_string())?;
            io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            drop(out);
            fs::rename(&part, &target).map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    #[test]
    #[ignore]
    fn latest_versions() {
        let v = super::latest_version(super::YTDLP_REPO);
        println!("yt-dlp latest: {v:?}");
        assert!(v.is_some_and(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit())));
        let v = super::latest_version(super::DENO_REPO);
        println!("deno latest: {v:?}");
        assert!(v.is_some_and(|v| !v.starts_with('v')));
    }
}
