# Windows desktop build via cross-compile (2026-10-22)

Up: [[Index]]

## Why
- The Windows work PC has no admin rights, so the Visual Studio Build Tools (needed by Rust's MSVC target) cannot be installed there. The Windows exe is cross-compiled on home2 instead and copied over.

## Setup on home2 (one time)
- `sudo apt-get install clang lld llvm` (LLVM 18 on Ubuntu 24.04).
- `rustup target add x86_64-pc-windows-msvc` (pinned toolchain 1.98.1).
- `cargo install --locked cargo-xwin` (0.23.1). On first build it downloads Microsoft's CRT and Windows SDK; `XWIN_ACCEPT_LICENSE=1` accepts that license.

## Build
- `XWIN_ACCEPT_LICENSE=1 CARGO_TARGET_DIR=$HOME/archaeodash-target bash scripts/build-windows-exe.sh`, run from the mirror `~/build/archaeodashdesktop` (the repo mount is noexec, so Vite's native binaries cannot run there; rsync the repo first, excluding `target`, `.git`, `apps/web/dist`).
- The script builds the web UI, then `cargo xwin build --release --target x86_64-pc-windows-msvc -p archaeodash-desktop-app --features tauri/custom-protocol` (the feature `tauri build` would set; serves the embedded frontend).
- Output: `~/archaeodash-target/x86_64-pc-windows-msvc/release/archaeodash-desktop.exe` (~29 MB, single file). First build ~9 min.

## Result
- GUI subsystem (no console), 7-size app icon embedded, WebView2 loader linked in (no `WebView2Loader.dll`).
- Needs the WebView2 runtime (ships with Windows 10/11) and the Visual C++ runtime (`VCRUNTIME140*.dll`); the work PC has both. For PCs without the VC++ runtime and no admin, a static-CRT build (`-C target-feature=+crt-static`) would remove that dependency.
- Installed on the work PC as `%LOCALAPPDATA%\Programs\ArchaeoDash\ArchaeoDash.exe`; it launched, opened the ArchaeoDash window, and logged a clean start to `%LOCALAPPDATA%\org.archaeodash.desktop\logs`.

Related: [[App_Icon_And_Branding_2026-10-22]], [[Interaction_Log_2026-10-22]]
