# Platforms

> Status: Windows (x64) and macOS (Apple Silicon) are supported. macOS on Intel should work (its downloads are in the table) but is untested. Linux is planned.

## Approach: download the tools at runtime

All tools publish standalone binaries, so every platform works the same way: dropdl downloads them into its data folder and keeps them up to date (see tooling.md).

| Tool | Windows | macOS | Linux (planned) |
|---|---|---|---|
| yt-dlp | `yt-dlp.exe` | `yt-dlp_macos.zip` (program folder, universal2) | `yt-dlp_linux`, `yt-dlp_linux_aarch64` |
| deno | `deno-x86_64-pc-windows-msvc.zip` | `deno-aarch64-apple-darwin.zip` / `deno-x86_64-apple-darwin.zip` | `deno-x86_64-unknown-linux-gnu.zip`, `aarch64` |
| ffmpeg, ffprobe | gyan.dev `ffmpeg-release-essentials.zip` (both in one zip) | martin-riedl.de `macos/{arm64,amd64}/release/{ffmpeg,ffprobe}.zip` (one zip each) | BtbN `linux64` / `linuxarm64` (static) |

On a platform without downloads (`tools::sources` returns nothing), dropdl uses tools found on `PATH`.

Alternatives that were rejected:
- **Bundle the tools inside the `.app`/AppImage:** 150–200 MB, and the bundle is read-only, so the tools could not update. yt-dlp has to stay current because sites change often.
- **Require system packages (brew, apt):** distribution packages of yt-dlp are often too old to work. A tool on `PATH` (including Homebrew's folders) stays as the fallback.

## Per-platform differences

| | Windows | macOS |
|---|---|---|
| Data folder | next to `dropdl.exe`, else `%LOCALAPPDATA%\dropdl` | next to the binary, except inside an `.app`: `~/Library/Application Support/dropdl` |
| yt-dlp in `bin/` | `yt-dlp.exe` | `yt-dlp/yt-dlp_macos` + `yt-dlp/_internal/` |
| Cancel | `taskkill /T /F` on the yt-dlp process tree | yt-dlp runs in its own process group (`process_group(0)`); `kill -KILL -<pgid>` |
| Show in folder | `explorer /select,` | `open -R` |
| CJK fallback fonts | `msyh.ttc`, `malgun.ttf` | `Hiragino Sans GB.ttc`, `AppleSDGothicNeo.ttc` |
| Paste hint | Ctrl+V | ⌘V (egui maps both to `Event::Paste`) |
| `tools::command()` | sets `CREATE_NO_WINDOW` | plain `Command` |

## Spike results (macOS)

Measured in October 2026 on Apple Silicon (macOS 27).

- **yt-dlp single file vs. program folder:** the single-file `yt-dlp_macos` (PyInstaller onefile, 37 MB) unpacks itself into a new temporary folder on every run, and macOS scans it each time: `--version` took ~10–11 s **on every run**. The program folder from `yt-dlp_macos.zip` (54 MB download, 175 MB unpacked) takes ~11 s on its first run only, then 0.18 s. dropdl uses the folder.
- **Gatekeeper:** files downloaded by dropdl itself (`ureq`) get no quarantine attribute, only `com.apple.provenance`. All four tools ran without a prompt.
- **ffmpeg (martin-riedl.de):** statically linked (only system frameworks), GPL with libx264, libmp3lame and AAC. The `redirect/latest/...` URL answers 307 with the version in the path (`/download/macos/arm64/1789931890_9.0.2/ffmpeg.zip`), so updates can be detected. A `HEAD` request returns 404; use `GET` without following the redirect. evermeet.cx's download URL now returns 410.
- **deno:** a single binary in a zip that keeps the executable bit.
- **PATH:** apps started from Finder get a minimal `PATH` (`/usr/bin:/bin:/usr/sbin:/sbin`), so Homebrew's `/opt/homebrew/bin` and `/usr/local/bin` are searched explicitly.

## Packaging

| Platform | Artifact | Notes |
|---|---|---|
| Windows | `dropdl.exe` | `scripts/build-release.ps1` (see release.md) |
| macOS (Apple Silicon) | `dropdl.app` in `dropdl-macos-arm64.zip` | `scripts/bundle-macos.sh`. Signed ad hoc (`codesign -s -`): without an Apple Developer ID and notarization, Gatekeeper blocks the first launch of a downloaded copy ("Open Anyway" in System Settings > Privacy & Security). Intel builds later (or a universal binary via `lipo`). |
| Linux (x86_64 first, planned) | AppImage + plain ELF in a `.tar.gz` | Build against an old glibc (Ubuntu 20.04 image or `cargo-zigbuild`). A static musl build isn't an option, because winit/glow load GL, X11 and Wayland libraries at runtime. Some distros need libfuse2 to run AppImages. |

Planned: builds on GitHub Actions (`windows-latest`, `macos-14` arm64, `ubuntu-22.04`).
