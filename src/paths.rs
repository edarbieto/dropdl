//! Where dropdl keeps its tools (`bin/`) and settings (`config/`).
//!
//! Portable by default: both folders live next to the executable, so the app
//! can run from a USB drive. If that location isn't writable (e.g. Program
//! Files, or inside a macOS .app bundle) it falls back to the per-user data dir.

use std::fs;
use std::path::{Path, PathBuf};

pub struct AppDirs {
    pub bin: PathBuf,
    pub config: PathBuf,
    pub portable: bool,
}

/// Resolves and creates the folders. `DROPDL_HOME` overrides the location.
pub fn resolve() -> AppDirs {
    if let Some(home) = std::env::var_os("DROPDL_HOME") {
        if let Some(dirs) = prepare(Path::new(&home), true) {
            return dirs;
        }
    }
    if let Some(dirs) = exe_dir().and_then(|dir| prepare(&dir, true)) {
        return dirs;
    }
    let base = dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("dropdl");
    prepare(&base, false).unwrap_or(AppDirs {
        bin: base.join("bin"),
        config: base.join("config"),
        portable: false,
    })
}

fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    // Writing inside a macOS bundle would break its code signature.
    let in_bundle = dir.ends_with("Contents/MacOS");
    (!in_bundle).then_some(dir)
}

/// Creates `base/bin` and `base/config` and checks they are writable.
fn prepare(base: &Path, portable: bool) -> Option<AppDirs> {
    let dirs = AppDirs {
        bin: base.join("bin"),
        config: base.join("config"),
        portable,
    };
    for dir in [&dirs.bin, &dirs.config] {
        fs::create_dir_all(dir).ok()?;
        let probe = dir.join(".write-test");
        fs::write(&probe, b"").ok()?;
        let _ = fs::remove_file(probe);
    }
    Some(dirs)
}
