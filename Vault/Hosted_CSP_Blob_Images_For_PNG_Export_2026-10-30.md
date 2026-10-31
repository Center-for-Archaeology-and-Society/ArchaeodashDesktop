Up: [[Index]]

# Hosted CSP allows blob: images for PNG plot export (2026-10-30)

## Problem
- "Save PNG" on Plotly charts failed on the hosted site.
- Plotly's PNG export loads the chart SVG as an `<img>` from a `blob:` URL (all browsers except Safari), then draws it on a canvas.
- The hosted CSP (`crates/api/src/security_headers.rs`) had `img-src 'self' data:`, so the browser blocked the image and the export failed.
- Hand-drawn SVG figures and SVG saves were not affected: they use `data:` URLs.
- The desktop app sets no CSP (`tauri.conf.json` `"csp": null`), so it was not affected. Its saves go through the native `save_export_file` command, which every app window may call.

## Fix
- `img-src 'self' data: blob:`. Blob URLs can only be created by scripts already running on the origin, so this adds no remote image source.
- The hosted test that lists the required CSP directives now expects `blob:`.

## Checks
- `scripts/home2/check.sh rust archaeodash-api`: 76 tests and clippy pass. `cargo fmt --check` flagged only `crates/application/tests/tmp_gwynn.rs`, an unrelated temporary file from other work in progress.
- Committed as `557400a`, with only these lines. `hosted.rs` also has uncommitted Project Trash changes ([[Hosted_Project_Trash_2026-10-30]]); they stay uncommitted and were not deployed.
- Deployed to cas as `cas-557400a` with `scripts/home2/deploy-cas.sh`. App healthy; public pages return 200. See [[Cas_Hosted_Deployment_2026-10-29]].
- On the live site: a `blob:` image that was blocked before the deploy now loads, and `Plotly.toImage(..., {format: 'png'})` returns a PNG.
- Not checked: clicking Save PNG on a real chart in a signed-in session, and saving from the Windows desktop app.
