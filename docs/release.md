# Release build

The goal is a single, portable executable that is as small as possible. This page covers Windows; the macOS bundle is described at the end.

```powershell
scripts/build-release.ps1          # → dist/dropdl.exe
scripts/build-release.ps1 -Upx     # also compress with UPX (upx on PATH or $env:UPX)
scripts/check-deps.ps1 <exe>       # verify only Windows system DLLs are imported
```

`build-release.ps1` builds with `cargo build --release`, runs `check-deps.ps1`, copies the exe to `dist/`, and optionally runs UPX.

## What makes it portable

- **Static C runtime** (`.cargo/config.toml`: `+crt-static`): no `VCRUNTIME140.dll`, so the Visual C++ Redistributable isn't needed.
- **Only system DLLs**: `check-deps.ps1` parses the PE import tables (regular and delay-load) and fails on anything not in `System32`.
- **glow (OpenGL) renderer**: uses `opengl32.dll`, which ships with Windows. No `d3dcompiler`/`dxcompiler`/`vulkan` DLLs are loaded.
- **Data next to the exe**: `bin/` and `config/` are created beside `dropdl.exe` (see tooling.md).

## Size measurements

Windows x64, Rust 1.95, eframe 0.36, measured on the same code. The startup time is the median of 5 launches, until the main window exists.

| Variant | Size | Notes |
|---|---:|---|
| A: wgpu renderer (eframe default), `opt-level=3` | 15.10 MB | original |
| B: glow | 9.52 MB | the renderer is the biggest lever |
| C: glow, no accesskit | 8.92 MB | |
| D: glow, `opt-level="s"` | 8.10 MB | |
| E: glow, `opt-level="z"` | 8.42 MB | `"z"` is *larger* than `"s"` here |
| F: glow, `"z"`, `panic="abort"` | 7.27 MB | |
| **G: glow, `"s"`, `panic="abort"`** | **6.87 MB** | **chosen release (shipped without UPX)** |
| H: G without accesskit | 6.53 MB | −0.34 MB, loses screen-reader support |
| G + UPX `--best --lzma` | 2.69 MB | |
| H + UPX `--best --lzma` | 2.58 MB | |

Startup time: ~31 ms uncompressed (A, G, H alike) vs. ~187 ms with UPX, because UPX decompresses into memory before running.

Windows Defender (`MpCmdRun -Scan -ScanType 3`) found no threats in G, G+UPX or H+UPX. Other antivirus engines were not tested. UPX-packed executables are a common source of false positives.

## Trade-offs

- **glow vs. wgpu:** glow needs OpenGL 2+ drivers. On machines without a GPU driver (some VMs, or Remote Desktop on older systems), wgpu could fall back to DX12 WARP and glow may fail to open a window. In exchange, glow works the same way on Windows, macOS and Linux.
- **`panic = "abort"`:** a panic in any thread closes the whole app instead of only that thread. The code avoids panics on expected errors, so this is acceptable for a much smaller binary.
- **UPX:** about 60% smaller (6.87 MB to 2.69 MB), at the cost of ~150 ms extra startup and a possible antivirus false positive.

## Publishing a release

Releases are built by GitHub Actions from a version tag, never from a local machine:

- `.github/workflows/ci.yml` runs `cargo test` on Windows and macOS for every push to `master` and every pull request.
- `.github/workflows/release.yml` runs on a `v*` tag. On `windows-latest` and `macos-14` (arm64) it checks that the tag matches the `Cargo.toml` version, runs `cargo test` and the release scripts (`scripts/build-release.ps1`, `scripts/bundle-macos.sh`; both fail on a non-system library). A last job adds `THIRD-PARTY-LICENSES.txt` and `SHA256SUMS.txt`, records [build provenance](https://docs.github.com/actions/security-for-github-actions/using-artifact-attestations) for both binaries, and creates a **draft** release with the version's `CHANGELOG.md` section as notes.

Both workflows pin the Rust version (`RUST_VERSION`), so sizes stay comparable; bump it on purpose.

Steps:

1. Bump `version` in `Cargo.toml` if needed, replace "(unreleased)" in the `CHANGELOG.md` heading with the date, and merge to `master`.
2. Tag and push: `git tag -a v0.3.0 -m "dropdl 0.3.0"` and `git push origin v0.3.0`.
3. Wait for the Release workflow, then review the draft on GitHub (files, notes) and click **Publish release**.

A tag with a suffix (`v0.3.0-rc.1`) makes a pre-release from the same version's notes, to try the workflow. Delete its draft and tag afterwards (`gh release delete v0.3.0-rc.1 --cleanup-tag`).

The release contains `dropdl-windows-x64.exe`, `dropdl-macos-arm64.zip` (with `THIRD-PARTY-LICENSES.txt` inside the app), `THIRD-PARTY-LICENSES.txt` and `SHA256SUMS.txt`. Anyone can check where a binary came from with `gh attestation verify <file> --repo edarbieto/dropdl`.

Don't ship the UPX build: packed executables, unsigned, from a downloader app are the most likely to be flagged by antivirus engines (see Trade-offs).

## Reproducing the measurements

Build each variant by editing the eframe features and `[profile.release]` in `Cargo.toml`. Then compare `target/release/dropdl.exe` sizes and run `upx --best --lzma` on a copy.

## macOS

```sh
scripts/bundle-macos.sh    # → dist/dropdl.app and dist/dropdl-macos-<arch>.zip
```

The script builds with `cargo build --release`, fails if the binary links a library outside `/usr/lib` and `/System/Library` (`otool -L`), writes `dropdl.app` with a minimal `Info.plist` (version from `Cargo.toml`), signs it ad hoc (`codesign --sign -`) and zips it with `ditto`.

Measured in October 2026 (Apple Silicon, variant G settings, Rust 1.99):

| Artifact | Size |
|---|---:|
| `dropdl.app/Contents/MacOS/dropdl` | 6.54 MB (5.95 MB before the music library code and `lofty`) |
| `dropdl-macos-arm64.zip` | 3.55 MB (3.43 MB before THIRD-PARTY-LICENSES.txt and the icon, 3.15 MB before the music library) |

Without an Apple Developer ID and notarization, Gatekeeper blocks the first launch of a copy downloaded with a browser; the user allows it once in System Settings > Privacy & Security ("Open Anyway").

## App icon

The icon (an orange download arrow in a circle, on a dark rounded square) comes from two SVG sources in `assets/`: `icon.svg` (full size, for Windows) and `icon-macos.svg` (the same design on Apple's icon grid: an 824 px body on a 1024 px canvas). `scripts/icons.sh` renders them with [resvg](https://github.com/linebender/resvg) (`cargo install resvg --locked`) into the checked-in files:

- `assets/icon.ico`: 16 to 256 px, embedded in `dropdl.exe` by `build.rs` with `winresource` (a Windows-only build dependency), together with the version metadata (version from `Cargo.toml`, product name and file description "dropdl", which Task Manager shows).
- `assets/icon-256.png`: the window and taskbar icon on Windows, set at runtime (`app_icon` in `src/main.rs`). eframe already links a PNG decoder for its default egui icon, so this adds no code.
- `assets/icon.icns`: copied into `dropdl.app` by `scripts/bundle-macos.sh` (`CFBundleIconFile`). On macOS `app_icon` returns an empty icon, so eframe keeps the bundle icon instead of replacing it in the Dock.

Rerun `scripts/icons.sh` (on a Mac) after changing an SVG.

## License notices

dropdl is `MIT OR Apache-2.0`. Its 217 linked crates for the release targets (checked October 2026) use compatible permissive licenses: MIT and/or Apache-2.0 for most, plus Unicode-3.0, ISC, Zlib, BSD-3-Clause, BSL-1.0, Unlicense and CDLA-Permissive-2.0 (`webpki-roots` data). Special cases:

- `self_cell` is `Apache-2.0 OR GPL-2.0-only`; it is used under Apache-2.0.
- `option-ext` (used by `dirs`) is MPL-2.0: file-level copyleft that only applies to changes to that crate.
- `epaint_default_fonts` embeds fonts under OFL-1.1 and the Ubuntu Font License, which allow embedding with their notice.

These licenses ask that a binary distribution includes the dependencies' copyright and license notices. They are collected in `THIRD-PARTY-LICENSES.txt` (checked in), generated from `Cargo.lock` with [cargo-about](https://github.com/EmbarkStudios/cargo-about):

```sh
cargo install cargo-about --locked --features cli   # once
cargo about generate about.hbs -o THIRD-PARTY-LICENSES.txt
```

`about.toml` lists the accepted licenses (generation fails on any other) and the release targets, and adds the notices of egui's embedded fonts that cargo-about doesn't find on its own (Hack's Bitstream Vera license among them). `scripts/build-release.ps1` copies the file to `dist/`, and `scripts/bundle-macos.sh` puts it in `dropdl.app/Contents/Resources/`. Regenerate it whenever `Cargo.lock` changes, and re-check new licenses. `lofty` (October 2026) added `lofty`, `lofty_attr`, `ogg_pager` and `paste` (MIT OR Apache-2.0), `data-encoding` (MIT) and `byteorder` (Unlicense OR MIT).

The tools dropdl downloads at runtime (yt-dlp, deno, the GPL ffmpeg build) are not distributed with dropdl and run as separate processes, so they don't affect its license. Bundling any of them in a release would bring their license terms (for ffmpeg: GPL source offer) into that release.
