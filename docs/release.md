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
| `dropdl.app/Contents/MacOS/dropdl` | 5.95 MB |
| `dropdl-macos-arm64.zip` | 3.15 MB |

Without an Apple Developer ID and notarization, Gatekeeper blocks the first launch of a copy downloaded with a browser; the user allows it once in System Settings > Privacy & Security ("Open Anyway"). The app has no icon yet (see ROADMAP).

## License notices

dropdl is `MIT OR Apache-2.0`. Its 204 linked crates (checked October 2026) use compatible permissive licenses: MIT and/or Apache-2.0 for most, plus Unicode-3.0, ISC, Zlib, BSD-2/3-Clause, BSL-1.0, 0BSD, Unlicense and CDLA-Permissive-2.0 (`webpki-roots` data). Special cases:

- `self_cell` is `Apache-2.0 OR GPL-2.0-only`; it is used under Apache-2.0.
- `option-ext` (used by `dirs`) is MPL-2.0: file-level copyleft that only applies to changes to that crate.
- `epaint_default_fonts` embeds fonts under OFL-1.1 and the Ubuntu Font License, which allow embedding with their notice.

These licenses ask that a binary distribution includes the dependencies' copyright and license notices. Before the first public release, generate a `THIRD-PARTY-LICENSES` file from `Cargo.lock` (e.g. with `cargo-about`) and ship it next to `dropdl.exe` and inside `dropdl.app`. Re-check when adding dependencies.

The tools dropdl downloads at runtime (yt-dlp, deno, the GPL ffmpeg build) are not distributed with dropdl and run as separate processes, so they don't affect its license. Bundling any of them in a release would bring their license terms (for ffmpeg: GPL source offer) into that release.
