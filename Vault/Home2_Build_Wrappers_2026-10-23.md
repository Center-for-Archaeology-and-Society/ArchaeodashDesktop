# home2 build wrappers and permission allowlist (2026-10-23)

Up: [[Index]]

## Why
- Builds, tests, and commits run on home2 over SSH. Claude Code's safety check asks for approval on every command sent to another machine, so routine build loops needed many manual approvals.

## What
- `scripts/home2/` — fixed wrappers run from the repo root in Git Bash on the Windows PC:
  - `sync.sh` — rsync the share into home2's build mirror (`~/build/archaeodashdesktop`).
  - `check.sh [all|rust|web|parity|fmt] [archaeodash-crate…]` — fmt check, tests, clippy (`-D warnings`, workspace minus the GTK-dependent Tauri app), web typecheck + tests; `fmt` applies `cargo fmt`. Exits non-zero on failure.
  - `build-windows.sh` — sync + cross-compile the Windows exe (`scripts/build-windows-exe.sh`, cargo-xwin).
  - `install-windows.sh [--launch]` — copy the build to `%LOCALAPPDATA%\Programs\ArchaeoDash\ArchaeoDash.exe` (keeps `ArchaeoDash.previous.exe`); refuses while the app runs.
  - `commit.sh MESSAGE_FILE PATH…` — commit on home2 (Windows git misreads modes over the share). Restores each changed file's recorded git mode (new files: 755 if they start with `#!`, else 644) before staging, commits as the previous commit's author, never pushes.
  - `preview.sh start|stop|status` — build and serve the web UI on `http://192.168.0.37:4173/`.
  - `common.sh` — shared host/paths and `run_remote`, which quotes arguments with `printf %q`.
- `.claude/settings.local.json` (personal, now gitignored) allows exactly these commands, so they run without prompts. Anything else on home2 still asks.

## Safety
- No wrapper accepts free-form commands: modes and flags come from fixed lists, crate names must match `archaeodash-[a-z0-9-]+`, commit paths must be repository-relative without `.`/`..` or leading `-` and must exist or be tracked, and the commit message travels as base64 data. Chained commands (`;`, `&&`) do not match the permission rules.
- A blanket `ssh home2 …` allow was rejected: home2 has passwordless sudo, so it would have allowed anything on that machine.

## Verification
- Argument validation refuses injected modes, crate names, flags, and paths locally (exit 2) before contacting home2.
- `sync.sh`, `check.sh web` (156/156), `check.sh rust archaeodash-desktop`, `preview.sh start/status/stop`, and `build-windows.sh` ran successfully; this note was committed with `commit.sh`.
- Not run: `install-windows.sh` (it would replace the installed app).
- The allow rules take effect in sessions started after the settings file was created (the settings watcher only covers directories that already had a settings file at session start).
