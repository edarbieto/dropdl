//! Detection, validation, download and update of the external binaries
//! dropdl depends on: yt-dlp, deno (JS runtime for YouTube) and ffmpeg/ffprobe.
//! Download sources per platform are listed in docs/tooling.md.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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

    /// Where dropdl keeps its own copy. On macOS yt-dlp is a program folder
    /// (the single-file build unpacks itself on every run, which takes ~10 s).
    fn local_path(self, bin: &Path) -> PathBuf {
        if cfg!(target_os = "macos") && self == Tool::YtDlp {
            bin.join(self.name()).join("yt-dlp_macos")
        } else {
            bin.join(self.exe())
        }
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

/// How a download turns into files in `bin`.
#[derive(Debug, PartialEq)]
enum Fetch {
    /// A bare executable.
    File(Tool),
    /// A zip; the tools' executables are extracted flat into `bin`.
    Zip(&'static [Tool]),
    /// A zip holding a whole program folder, extracted to `bin/<tool name>/`.
    Folder(Tool),
}

/// Tools that are installed and updated together, from one or more downloads.
struct Source {
    tools: &'static [Tool],
    /// URL whose redirect names the latest version (see [`version_in`]).
    latest: String,
    downloads: Vec<(String, Fetch)>,
}

const FFMPEG_PAIR: &[Tool] = &[Tool::Ffmpeg, Tool::Ffprobe];

/// Download sources for an OS and CPU (`std::env::consts` values). Empty on
/// platforms without downloads: tools on PATH are used there.
fn sources(os: &str, arch: &str) -> Vec<Source> {
    let gh_latest = |repo: &str| format!("https://github.com/{repo}/releases/latest");
    let gh_asset = |repo: &str, file: &str| format!("https://github.com/{repo}/releases/latest/download/{file}");
    let deno = |target: &str| {
        let url = gh_asset("denoland/deno", &format!("deno-{target}.zip"));
        Source { tools: &[Tool::Deno], latest: gh_latest("denoland/deno"), downloads: vec![(url, Fetch::Zip(&[Tool::Deno]))] }
    };
    match os {
        "windows" => {
            let ffmpeg = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip".to_string();
            vec![
                Source {
                    tools: &[Tool::YtDlp],
                    latest: gh_latest("yt-dlp/yt-dlp"),
                    downloads: vec![(gh_asset("yt-dlp/yt-dlp", "yt-dlp.exe"), Fetch::File(Tool::YtDlp))],
                },
                deno("x86_64-pc-windows-msvc"),
                Source { tools: FFMPEG_PAIR, latest: ffmpeg.clone(), downloads: vec![(ffmpeg, Fetch::Zip(FFMPEG_PAIR))] },
            ]
        }
        "macos" => {
            let (deno_arch, ff_arch) = if arch == "aarch64" { ("aarch64", "arm64") } else { ("x86_64", "amd64") };
            let ff = |tool: Tool| format!("https://ffmpeg.martin-riedl.de/redirect/latest/macos/{ff_arch}/release/{}.zip", tool.name());
            vec![
                Source {
                    tools: &[Tool::YtDlp],
                    latest: gh_latest("yt-dlp/yt-dlp"),
                    downloads: vec![(gh_asset("yt-dlp/yt-dlp", "yt-dlp_macos.zip"), Fetch::Folder(Tool::YtDlp))],
                },
                deno(&format!("{deno_arch}-apple-darwin")),
                Source {
                    tools: FFMPEG_PAIR,
                    latest: ff(Tool::Ffmpeg),
                    downloads: vec![
                        (ff(Tool::Ffmpeg), Fetch::Zip(&[Tool::Ffmpeg])),
                        (ff(Tool::Ffprobe), Fetch::Zip(&[Tool::Ffprobe])),
                    ],
                },
            ]
        }
        _ => Vec::new(),
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
/// missing ones and updates them when a newer release exists.
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

    let sources = sources(std::env::consts::OS, std::env::consts::ARCH);
    for src in &sources {
        let current: Option<Vec<&str>> =
            src.tools.iter().map(|&t| local[t as usize].as_ref().map(|(_, v)| v.as_str())).collect();
        if let Some(current) = current {
            let name = src.tools[0].name();
            match latest_version(&src.latest) {
                Some(latest) if current.iter().any(|v| comparable(v) != latest) => {
                    log(format!("{name}: updating {} → {latest}", current[0]))
                }
                Some(_) => continue,
                None => {
                    log(format!("{name}: could not check for updates"));
                    continue;
                }
            }
        }
        install(src.tools, bin, &report, &log, |p| fetch_all(&src.downloads, bin, p));
    }

    // No download for this platform: fall back to PATH.
    for tool in Tool::ALL {
        let covered = sources.iter().any(|s| s.tools.contains(&tool));
        if !covered && local[tool as usize].is_none() {
            install(&[tool], bin, &report, &log, |_| Err("no download for this platform".into()));
        }
    }
}

/// dropdl's own copy of `tool`, if it runs.
fn local(bin: &Path, tool: Tool) -> Option<(PathBuf, String)> {
    let path = tool.local_path(bin);
    let v = version(tool, &path)?;
    Some((path, v))
}

/// The first working copy of `tool` on PATH. Apps started from Finder get a
/// minimal PATH, so Homebrew's folders are searched too.
fn on_path(tool: Tool) -> Option<(PathBuf, String)> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    dirs.into_iter()
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
    // "2026.08.19" / "deno 2.5.0 (stable, ...)" / "ffmpeg version 9.0.2-essentials_build-www.gyan.dev Copyright ..."
    let v = match tool {
        Tool::YtDlp => first,
        Tool::Deno => first.split_whitespace().nth(1).unwrap_or(first),
        Tool::Ffmpeg | Tool::Ffprobe => first.split_whitespace().nth(2).unwrap_or(first),
    };
    Some(v.to_string())
}

/// The part of a local version that release versions are compared with:
/// ffmpeg builds append their origin ("9.0.2-essentials_build-www.gyan.dev").
fn comparable(version: &str) -> &str {
    version.split('-').next().unwrap_or(version)
}

/// Latest version behind `url`, read from its redirect without following it.
/// One small request, which doesn't count against GitHub's API rate limit.
fn latest_version(url: &str) -> Option<String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let resp = agent.get(url).call().ok()?;
    let location = resp.headers().get("location")?.to_str().ok()?;
    version_in(location)
}

/// The first version number (digits and dots, with at least one dot) in a
/// redirect target: `…/releases/tag/v2.5.0`, `…/1789931890_9.0.2/ffmpeg.zip`,
/// `…/ffmpeg-9.0.2-essentials_build.zip`.
fn version_in(location: &str) -> Option<String> {
    location
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|s| s.trim_matches('.'))
        .find(|s| s.contains('.') && s.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_string)
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

/// Runs a source's downloads in order, with one combined progress.
fn fetch_all(downloads: &[(String, Fetch)], bin: &Path, progress: &mut dyn FnMut(f32)) -> Result<(), String> {
    let n = downloads.len() as f32;
    for (i, (url, fetch)) in downloads.iter().enumerate() {
        let mut p = |f: f32| progress((i as f32 + f) / n);
        match fetch {
            Fetch::File(tool) => {
                let dest = bin.join(tool.exe());
                download(url, &dest, &mut p)?;
                make_executable(&dest)
            }
            Fetch::Zip(tools) => {
                let names: Vec<String> = tools.iter().map(|t| t.exe()).collect();
                let names: Vec<&str> = names.iter().map(String::as_str).collect();
                download_zip(url, bin, &names, &mut p)
            }
            Fetch::Folder(tool) => download_folder(url, bin, tool.name(), &mut p),
        }?;
    }
    Ok(())
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
    let result = extract_flat(&zip_path, dest, wanted);
    let _ = fs::remove_file(&zip_path);
    result
}

fn extract_flat(zip_path: &Path, dest: &Path, wanted: &[&str]) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
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
        make_executable(&part)?;
        fs::rename(&part, &target).map_err(|e| e.to_string())?;
        found += 1;
    }
    if found < wanted.len() {
        return Err(format!("archive is missing {}", wanted.join(", ")));
    }
    Ok(())
}

/// Downloads a zip with a program folder and replaces `bin/<name>/` with it.
/// The old folder is only removed once the new one is complete.
fn download_folder(url: &str, bin: &Path, name: &str, progress: &mut dyn FnMut(f32)) -> Result<(), String> {
    let zip_path = bin.join("download.zip");
    download(url, &zip_path, progress)?;
    let part = bin.join(format!("{name}.part"));
    let _ = fs::remove_dir_all(&part);
    let result = extract_tree(&zip_path, &part).and_then(|()| replace_dir(&part, &bin.join(name)));
    let _ = fs::remove_file(&zip_path);
    let _ = fs::remove_dir_all(&part);
    result
}

/// Extracts every entry of a zip below `dest`, keeping Unix permissions and symlinks.
fn extract_tree(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let file = fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("unsafe path in archive: {}", entry.name().unwrap_or_default()));
        };
        let target = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if entry.is_symlink() {
            let mut link = String::new();
            entry.read_to_string(&mut link).map_err(|e| e.to_string())?;
            symlink(&link, &target)?;
            continue;
        }
        let mut out = fs::File::create(&target).map_err(|e| e.to_string())?;
        io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(mode & 0o777)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Swaps `new` into `dest`. If the swap fails, the old folder is restored.
fn replace_dir(new: &Path, dest: &Path) -> Result<(), String> {
    let old = dest.with_extension("old");
    let _ = fs::remove_dir_all(&old);
    let had_old = dest.exists();
    if had_old {
        fs::rename(dest, &old).map_err(|e| e.to_string())?;
    }
    if let Err(e) = fs::rename(new, dest) {
        if had_old {
            let _ = fs::rename(&old, dest);
        }
        return Err(e.to_string());
    }
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

#[cfg(unix)]
fn symlink(link: &str, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(link, target).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn symlink(_link: &str, target: &Path) -> Result<(), String> {
    Err(format!("symlinks are not supported: {}", target.display()))
}

fn make_executable(_path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(_path, fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_from_redirects() {
        let cases = [
            ("https://github.com/yt-dlp/yt-dlp/releases/tag/2026.10.21", "2026.10.21"),
            ("https://github.com/denoland/deno/releases/tag/v2.9.8", "2.9.8"),
            ("/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip", "9.0.2"),
            ("https://www.gyan.dev/ffmpeg/builds/packages/ffmpeg-9.0.2-essentials_build.zip", "9.0.2"),
        ];
        for (location, want) in cases {
            assert_eq!(version_in(location).as_deref(), Some(want), "{location}");
        }
        assert_eq!(version_in("https://example.com/no/version"), None);
    }

    #[test]
    fn comparable_versions() {
        assert_eq!(comparable("9.0.2-essentials_build-www.gyan.dev"), "9.0.2");
        assert_eq!(comparable("9.0.2-https://www.martin-riedl.de"), "9.0.2");
        assert_eq!(comparable("2026.10.21"), "2026.10.21");
        // Old BtbN master builds never equal a release, so they get replaced.
        assert_ne!(comparable("N-121345-gabcdef0123-20260901"), "9.0.2");
    }

    #[test]
    fn platform_sources() {
        let all = |os, arch| {
            let s = sources(os, arch);
            let mut tools: Vec<Tool> = s.iter().flat_map(|s| s.tools.iter().copied()).collect();
            tools.dedup();
            assert_eq!(tools, Tool::ALL, "{os} {arch}");
            s
        };
        let win = all("windows", "x86_64");
        assert_eq!(win[0].downloads[0].1, Fetch::File(Tool::YtDlp));
        assert!(win[1].downloads[0].0.ends_with("deno-x86_64-pc-windows-msvc.zip"));
        assert!(win[2].downloads[0].0.contains("gyan.dev"));

        let mac = all("macos", "aarch64");
        assert_eq!(mac[0].downloads[0].1, Fetch::Folder(Tool::YtDlp));
        assert!(mac[0].downloads[0].0.ends_with("yt-dlp_macos.zip"));
        assert!(mac[1].downloads[0].0.ends_with("deno-aarch64-apple-darwin.zip"));
        assert_eq!(mac[2].downloads.len(), 2);
        assert!(mac[2].downloads[1].0.ends_with("/macos/arm64/release/ffprobe.zip"));

        let intel = all("macos", "x86_64");
        assert!(intel[1].downloads[0].0.ends_with("deno-x86_64-apple-darwin.zip"));
        assert!(intel[2].downloads[0].0.contains("/macos/amd64/"));

        assert!(sources("linux", "x86_64").is_empty());
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dropdl-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A zip like yt-dlp's macOS build: an executable next to an `_internal/` folder.
    fn write_zip(path: &Path) {
        use zip::write::SimpleFileOptions;
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.add_directory("_internal/", opts).unwrap();
        zip.start_file("_internal/lib.txt", opts.unix_permissions(0o644)).unwrap();
        zip.write_all(b"lib").unwrap();
        zip.start_file("tool", opts.unix_permissions(0o755)).unwrap();
        zip.write_all(b"#!/bin/sh\necho 1.0\n").unwrap();
        zip.add_symlink("current", "tool", opts).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn folder_install_and_update() {
        let bin = temp_dir("folder");
        let zip = bin.join("t.zip");
        write_zip(&zip);
        let part = bin.join("tool.part");
        let dest = bin.join("tool");

        // Fresh install, then an update over the existing folder.
        for _ in 0..2 {
            extract_tree(&zip, &part).unwrap();
            replace_dir(&part, &dest).unwrap();
        }
        assert!(!part.exists() && !bin.join("tool.old").exists());
        assert_eq!(fs::read_to_string(dest.join("_internal/lib.txt")).unwrap(), "lib");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&dest.join("tool")), 0o755);
            assert_eq!(mode(&dest.join("_internal/lib.txt")), 0o644);
            assert_eq!(fs::read_link(dest.join("current")).unwrap(), Path::new("tool"));
        }

        extract_flat(&zip, &bin, &["tool.txt"]).unwrap_err();
        let _ = fs::remove_dir_all(&bin);
    }

    #[test]
    #[ignore]
    fn latest_versions() {
        for src in sources(std::env::consts::OS, std::env::consts::ARCH) {
            let v = latest_version(&src.latest);
            println!("{} latest: {v:?}", src.tools[0].name());
            assert!(v.is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit())));
        }
    }
}
