# Interaction Log — 2026-09-29

- Requested that desktop source import open a native file picker rooted at the active project; added the optional Tauri file service picker, shared staged-file preview path, desktop button, browser input fallback, and coverage for cancellation and both surfaces. The Rust picker command starts each dialog at the active project root and stages supported files safely. Typechecks, the source-name unit test, and 94 web tests pass using the cached pnpm module. See [[Desktop_Import_Source_Native_Picker_2026-09-29]].
