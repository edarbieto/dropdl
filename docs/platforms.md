# macOS and Linux (planned)

> Status: design, not implemented yet. Windows is the only supported platform today.

## Approach: download the tools at runtime, as on Windows

All tools publish standalone binaries, so the Windows model carries over: dropdl downloads them into its data folder and keeps yt-dlp and deno up to date.

| Tool | macOS | Linux |
|---|---|---|
| yt-dlp | `yt-dlp_macos` (universal2) | `yt-dlp_linux`, `yt-dlp_linux_aarch64` |
| deno | `deno-aarch64-apple-darwin.zip` (`x86_64` for Intel) | `deno-x86_64-unknown-linux-gnu.zip`, `aarch64` |
| ffmpeg, ffprobe | BtbN has no macOS builds; use another source with a stable "latest" URL (e.g. martin-riedl.de, arm64 and Intel) | BtbN `linux64` / `linuxarm64` (static) |

Alternatives that were rejected:
- **Bundle the tools inside the `.app`/AppImage:** 150–200 MB, and the bundle is read-only, so yt-dlp could not update itself. yt-dlp has to stay current because sites change often.
- **Require system packages (brew, apt):** distribution packages of yt-dlp are often too old to work. A tool on `PATH` stays as the fallback it already is.

## Per-platform work

- **Tool URLs** chosen by OS and `std::env::consts::ARCH`. Set the executable bit after download, and extract `.tar.xz` (BtbN Linux) as well as `.zip`.
- **Data folder:** bundles are read-only, so `paths.rs` falls back to the per-user data dir (`~/Library/Application Support/dropdl`, `~/.local/share/dropdl`). For an AppImage, the folder next to the `.AppImage` file (`$APPIMAGE`) can be the portable location.
- **Cancel:** spawn yt-dlp in its own process group and kill the group (`kill -KILL -<pgid>`). The PyInstaller yt-dlp is a launcher with a child process, like on Windows.
- **Reveal in folder:** `open -R <file>` on macOS, `xdg-open <dir>` on Linux.
- **Fallback fonts (CJK):** PingFang / Hiragino on macOS, Noto CJK on Linux (common font paths).
- **`tools::command()`:** `CREATE_NO_WINDOW` is Windows-only. Elsewhere it is a plain `Command`.

## Packaging

| Platform | Artifact | Notes |
|---|---|---|
| macOS (Apple Silicon first) | `dropdl.app` in a `.zip` or `.dmg` | Without an Apple Developer ID and notarization, Gatekeeper blocks the first launch ("Open Anyway" in System Settings). Ad-hoc signing (`codesign -s -`) is the minimum. Tools that dropdl downloads itself are not quarantined. Intel builds later (or a universal binary via `lipo`). |
| Linux (x86_64 first) | AppImage + plain ELF in a `.tar.gz` | Build against an old glibc (Ubuntu 20.04 image or `cargo-zigbuild`). A static musl build isn't an option, because winit/glow load GL, X11 and Wayland libraries at runtime. Some distros need libfuse2 to run AppImages. |

Builds run on GitHub Actions (`macos-14` arm64, `ubuntu-22.04`). macOS can't practically be cross-compiled from Windows. Manual testing: Apple Silicon Mac and a Linux PC/VM.
