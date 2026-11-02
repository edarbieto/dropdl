//! Updates dropdl itself from its GitHub releases: the new build is checked
//! against the release's SHA256SUMS.txt, swapped in next to the running app
//! and started when the old one closes.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools;

const RELEASES: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases");
const SUMS: &str = "SHA256SUMS.txt";
/// Next to the app, because a rename only works within one volume.
const STAGING: &str = ".dropdl-update";
const OLD: &str = ".dropdl-old";

pub enum Event {
    Available(String),
    Progress(f32),
    /// The new build is in place at this path.
    Done(Result<PathBuf, String>),
}

/// The latest published version, if it's newer than this one. Drafts and
/// pre-releases don't count: GitHub's latest link skips them.
pub fn check() -> Option<String> {
    let latest = tools::latest_version(&format!("{RELEASES}/latest"))?;
    is_newer(&latest, env!("CARGO_PKG_VERSION")).then_some(latest)
}

pub fn release_page(version: &str) -> String {
    format!("{RELEASES}/tag/v{version}")
}

/// Removes what the previous update left. It runs late because on Windows the
/// old exe can't be deleted until that process has ended.
pub fn clear() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(2));
        if let Some(folder) = install_path().ok().as_deref().and_then(Path::parent) {
            remove(&folder.join(OLD));
            remove(&folder.join(STAGING));
        }
    });
}

/// Downloads `version` and puts it where this app is. The running app keeps
/// working from its renamed copy until it closes.
pub fn install(version: &str, progress: &mut dyn FnMut(f32)) -> Result<PathBuf, String> {
    let asset = asset(std::env::consts::OS, std::env::consts::ARCH).ok_or("no build for this platform")?;
    let target = install_path()?;
    let folder = target.parent().ok_or("the app has no folder")?;
    let staging = folder.join(STAGING);
    remove(&staging);
    fs::create_dir(&staging).map_err(|e| format!("{}: {e}", folder.display()))?;
    let result = (|| {
        let base = format!("{RELEASES}/download/v{version}");
        let file = staging.join(asset);
        let sums = staging.join(SUMS);
        tools::download(&format!("{base}/{asset}"), &file, progress)?;
        tools::download(&format!("{base}/{SUMS}"), &sums, &mut |_| {})?;
        let sums = fs::read_to_string(&sums).map_err(|e| e.to_string())?;
        verify(&file, asset, &sums)?;
        replace(&unpack(&file, &staging)?, &target, &folder.join(OLD))
    })();
    remove(&staging);
    result.map(|()| target)
}

/// Starts the build at `path`. `open -n` because the old app may still be
/// running, and macOS would only bring that one to the front.
pub fn relaunch(path: &Path) -> Result<(), String> {
    let mut cmd = if cfg!(target_os = "macos") {
        let mut open = tools::command("/usr/bin/open");
        open.arg("-n").arg(path);
        open
    } else {
        tools::command(path)
    };
    cmd.spawn().map(drop).map_err(|e| e.to_string())
}

/// The release file for an OS and CPU (`std::env::consts` values).
fn asset(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") => Some("dropdl-windows-x64.exe"),
        ("macos", "aarch64") => Some("dropdl-macos-arm64.zip"),
        _ => None,
    }
}

/// Compares dotted numbers, so 0.3.10 is newer than 0.3.9.
fn is_newer(latest: &str, current: &str) -> bool {
    let numbers = |v: &str| v.split('.').map(str::parse).collect::<Result<Vec<u64>, _>>().ok();
    matches!((numbers(latest), numbers(current)), (Some(l), Some(c)) if l > c)
}

/// What an update replaces: the `.app` on macOS, the exe elsewhere.
fn install_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        return bundle_of(&exe).map(Path::to_path_buf).ok_or_else(|| "dropdl isn't running from an app bundle".to_string());
    }
    Ok(exe)
}

fn bundle_of(exe: &Path) -> Option<&Path> {
    let app = exe.parent()?.parent()?.parent()?;
    (exe.parent()?.ends_with("Contents/MacOS") && app.extension()? == "app").then_some(app)
}

/// The SHA-256 of `name` in a checksum file (`<hash>  <file>` lines).
fn expected_sum<'a>(sums: &'a str, name: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim_start().trim_start_matches('*') == name).then_some(hash)
    })
}

/// Catches a broken download. It's no defense against a replaced release,
/// because the checksums come from the same place.
fn verify(file: &Path, name: &str, sums: &str) -> Result<(), String> {
    let want = expected_sum(sums, name).ok_or_else(|| format!("{name} isn't in {SUMS}"))?;
    let bytes = fs::read(file).map_err(|e| e.to_string())?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let got: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if got.eq_ignore_ascii_case(want) { Ok(()) } else { Err(format!("{name} doesn't match its checksum")) }
}

/// The downloaded build, ready to be moved into place.
fn unpack(file: &Path, staging: &Path) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        tools::make_executable(file)?;
        return Ok(file.to_path_buf());
    }
    run(tools::command("/usr/bin/ditto").args(["-x", "-k"]).arg(file).arg(staging))?;
    let app = staging.join("dropdl.app");
    run(tools::command("/usr/bin/codesign").arg("--verify").arg(&app))?;
    Ok(app)
}

fn run(cmd: &mut std::process::Command) -> Result<(), String> {
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!("{}: {}", cmd.get_program().to_string_lossy(), String::from_utf8_lossy(&out.stderr).trim()))
}

/// Swaps `new` into `target` and keeps the old one at `old`, as a running exe
/// can be renamed but not deleted. If the swap fails, the old one is restored.
fn replace(new: &Path, target: &Path, old: &Path) -> Result<(), String> {
    remove(old);
    fs::rename(target, old).map_err(|e| format!("{}: {e}", target.display()))?;
    if let Err(e) = fs::rename(new, target) {
        let _ = fs::rename(old, target);
        return Err(format!("{}: {e}", target.display()));
    }
    Ok(())
}

fn remove(path: &Path) {
    let _ = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dropdl-test-update-{name}-{}", std::process::id()));
        remove(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn newer_versions() {
        assert!(is_newer("0.3.2", "0.3.1"));
        assert!(is_newer("0.3.10", "0.3.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.3.1", "0.3.1"));
        assert!(!is_newer("0.3.0", "0.3.1"));
        assert!(!is_newer("0.4.0-rc.1", "0.3.1"));
        assert!(!is_newer("nonsense", "0.3.1"));
    }

    #[test]
    fn assets_per_platform() {
        assert_eq!(asset("windows", "x86_64"), Some("dropdl-windows-x64.exe"));
        assert_eq!(asset("macos", "aarch64"), Some("dropdl-macos-arm64.zip"));
        assert_eq!(asset("macos", "x86_64"), None);
        assert_eq!(asset("linux", "x86_64"), None);
    }

    #[test]
    fn app_bundles() {
        let bundle = |p: &str| bundle_of(Path::new(p)).map(Path::to_path_buf);
        assert_eq!(bundle("/Applications/dropdl.app/Contents/MacOS/dropdl"), Some("/Applications/dropdl.app".into()));
        assert_eq!(bundle("/Apps/renamed.app/Contents/MacOS/dropdl"), Some("/Apps/renamed.app".into()));
        assert_eq!(bundle("/project/target/debug/dropdl"), None);
        assert_eq!(bundle("/x/Contents/MacOS/dropdl"), None);
    }

    #[test]
    fn checksums() {
        let dir = temp_dir("sums");
        let file = dir.join("build.zip");
        fs::write(&file, "hello").unwrap();
        let hash = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

        let sums = format!("{}  other.exe\n{hash}  build.zip\n", "0".repeat(64));
        assert_eq!(expected_sum(&sums, "build.zip"), Some(hash));
        verify(&file, "build.zip", &sums).unwrap();
        // sha256sum marks binary files with a star.
        verify(&file, "build.zip", &format!("{hash} *build.zip\n")).unwrap();
        verify(&file, "build.zip", &format!("{}  build.zip\n", hash.to_uppercase())).unwrap();

        fs::write(&file, "hellp").unwrap();
        assert!(verify(&file, "build.zip", &sums).unwrap_err().contains("checksum"));
        assert!(verify(&file, "gone.zip", &sums).unwrap_err().contains("isn't in"));
        remove(&dir);
    }

    #[test]
    fn replaces_a_folder_and_keeps_the_old_one() {
        let dir = temp_dir("folder");
        let (new, target, old) = (dir.join("new.app"), dir.join("dropdl.app"), dir.join(OLD));
        fs::create_dir(&new).unwrap();
        fs::write(new.join("v"), "2").unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("v"), "1").unwrap();
        // Left by an update whose cleanup never ran.
        fs::create_dir(&old).unwrap();

        replace(&new, &target, &old).unwrap();

        assert_eq!(fs::read_to_string(target.join("v")).unwrap(), "2");
        assert_eq!(fs::read_to_string(old.join("v")).unwrap(), "1");
        assert!(!new.exists());
        remove(&dir);
    }

    #[test]
    fn replaces_a_file() {
        let dir = temp_dir("file");
        let (new, target, old) = (dir.join("new.exe"), dir.join("dropdl.exe"), dir.join(OLD));
        fs::write(&new, "2").unwrap();
        fs::write(&target, "1").unwrap();

        replace(&new, &target, &old).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "2");
        assert_eq!(fs::read_to_string(&old).unwrap(), "1");
        remove(&dir);
    }

    #[test]
    fn failed_swap_restores_the_old_build() {
        let dir = temp_dir("restore");
        let (target, old) = (dir.join("dropdl.exe"), dir.join(OLD));
        fs::write(&target, "1").unwrap();

        assert!(replace(&dir.join("missing"), &target, &old).is_err());

        assert_eq!(fs::read_to_string(&target).unwrap(), "1");
        assert!(!old.exists());
        remove(&dir);
    }

    /// A folder nobody may write in stops the update before anything changes.
    #[cfg(unix)]
    #[test]
    fn read_only_folder_changes_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("locked");
        let (new, locked) = (dir.join("new"), dir.join("locked"));
        fs::write(&new, "2").unwrap();
        fs::create_dir(&locked).unwrap();
        let target = locked.join("dropdl");
        fs::write(&target, "1").unwrap();
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();

        let result = replace(&new, &target, &locked.join(OLD));

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&target).unwrap(), "1");
        remove(&dir);
    }

    #[test]
    #[ignore]
    fn live_latest_release() {
        let latest = tools::latest_version(&format!("{RELEASES}/latest")).unwrap();
        println!("latest: {latest}");
        let name = asset(std::env::consts::OS, std::env::consts::ARCH).unwrap();
        let dir = temp_dir("live");
        let base = format!("{RELEASES}/download/v{latest}");
        let (file, sums) = (dir.join(name), dir.join(SUMS));
        tools::download(&format!("{base}/{name}"), &file, &mut |_| {}).unwrap();
        tools::download(&format!("{base}/{SUMS}"), &sums, &mut |_| {}).unwrap();
        verify(&file, name, &fs::read_to_string(&sums).unwrap()).unwrap();
        remove(&dir);
    }
}
