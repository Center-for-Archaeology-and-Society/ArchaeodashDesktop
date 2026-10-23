# App icon and in-app branding (2026-10-22)

Up: [[Index]]

## What
- Desktop icons generated from the repo-root `logo.png` and listed in `apps/desktop/src-tauri/tauri.conf.json` (`bundle.icon`), replacing the 1x1 placeholder `icon.png` and the old `icon.ico`.
- Web UI: favicon, apple-touch icon, and theme colour in `apps/web/index.html`; the AD mark beside the "ArchaeoDash" name in the top nav; the full logo on the Home page. Assets live in `apps/web/public/` (served from the site root in hosted mode and in the Tauri bundle).

## Artwork rules
- Two artworks: the full logo (AD over the element table) for 64 px and up, and a letters-only AD mark for 48 px and below, where the table lines and digits turn to noise. The mark is cut out by a morphological opening of the alpha mask.
- Stray colour under fully transparent source pixels is cleared so it cannot bleed into resized edges.
- Square icons keep a 4% margin; small sizes get a light sharpen.

## Files and sizes
- `icons/`: `32x32.png` (mark), `64x64.png`, `128x128.png`, `128x128@2x.png` (256), `icon.png` (512; first PNG in `bundle.icon`, so it is the Linux window icon), `icon.ico` (16/24/32/48 mark + 64/128/256 full; Windows window and exe icon), `icon.icns` (16–1024 incl. @2x entries).
- `public/`: `favicon.ico` (16/32/48), `apple-touch-icon.png` (180, opaque), `brand-mark.png` + `@2x` + `@3x` (28 CSS px tall), `logo-256.png` / `logo-512.png` (Home logo, 160 CSS px via `srcset`).

## Regenerating
- `bash scripts/generate-icons.sh [logo.png]` (ImageMagick 6 or 7 plus python3 for `.icns`). Run it on a host with those tools (home2 has them).

## Verification
- Web typecheck clean; web tests 128/128 (shell tests assert the nav mark and Home logo `srcset`).
- Vite production build copies all public assets into `dist`; previewed the build in a browser in the Simple and Dark themes; favicon served.
- Not checked: the Tauri window/taskbar icon in a rebuilt desktop app (needs a full desktop build).

## Windows taskbar icon fix (2026-10-23)
- The taskbar button looked pixelated: Tauri sets the window icon from one image of `icon.ico`, and Windows stretched that single bitmap to the taskbar size.
- `apps/desktop/src-tauri/src/windows_icon.rs` loads the exe's embedded multi-size icon (tauri-build's resource id 32512, else the first icon group) with `LoadImageW` at the window DPI's `SM_CXICON`/`SM_CXSMICON` and sets it with `WM_SETICON` (big and small); re-applied on scale-factor changes. Windows then uses the hand-made 16/24/32/48 px mark entries.
- The shell crate's lints copy the workspace lints but deny (not forbid) unsafe code, so this one module can opt in. `windows-sys` 0.59 (already in the lockfile) is a Windows-only dependency.
- Possible follow-up: add 20 and 40 px entries to the `.ico` (125% scaling) with `scripts/generate-icons.sh`.

Related: [[Interaction_Log_2026-10-22]], [[Interaction_Log_2026-10-23]]
