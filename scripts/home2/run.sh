#!/usr/bin/env bash
# Runs one named action on home2. Covers the routine home2 work the other
# wrappers don't, so it no longer needs a hand-written `ssh home2 ...`
# (which always asks for approval). Only the actions below exist; there is
# no way to pass a free-form command.
#
# Usage: bash scripts/home2/run.sh ACTION [ARGS...]
#   status               git log (last 3), short status, and mode changes in
#                        the home2 repository (read-only)
#   test CRATE [FILTER]  cargo test for one archaeodash-* crate, optionally
#                        only tests whose names contain FILTER, with test
#                        output shown (--nocapture); last 80 lines
#   clippy-desktop       sync the mirror, then clippy (-D warnings) on the
#                        Tauri desktop app for the Windows target (check.sh
#                        skips it because it needs GTK on Linux)
#   preview-log          last 40 lines of the preview server log
#
# Exits non-zero if the action fails.
set -euo pipefail
. "$(dirname "$0")/common.sh"
usage="usage: bash scripts/home2/run.sh status|test CRATE [FILTER]|clippy-desktop|preview-log"

action="${1:-}"
[ "$#" -gt 0 ] && shift
case "$action" in
  status|clippy-desktop|preview-log)
    [ "$#" -eq 0 ] || die "$usage" ;;
  test)
    [ "$#" -ge 1 ] && [ "$#" -le 2 ] || die "$usage"
    [[ "$1" =~ ^archaeodash-[a-z0-9-]+$ ]] || die "invalid crate name '$1'"
    [ "$#" -eq 1 ] || [[ "$2" =~ ^[A-Za-z0-9_:]+$ ]] || die "invalid test filter '$2' (letters, digits, _ and : only)" ;;
  *) die "$usage" ;;
esac

case "$action" in
  status)
    run_remote '
set -euo pipefail
cd '"$HOME2_REPO"'
g() { git -c safe.directory="*" "$@"; }
g log --oneline -3 | cat
echo "-- status --"
g status --short | head -40
echo "-- mode changes --"
g diff --summary | head -40' ;;
  test)
    run_remote '
set -uo pipefail
cd '"$HOME2_REPO"'
export CARGO_TARGET_DIR='"$HOME2_TARGET"'
out=$(cargo test -p "$@" -- --nocapture 2>&1); rc=$?
printf "%s\n" "$out" | grep -vE "^\s+(Compiling|Checking|Running|Downloaded|Downloading) " | tail -80
exit $rc' "$@" ;;
  clippy-desktop)
    run_remote "set -eo pipefail
mkdir -p $HOME2_MIRROR
$SYNC_SNIPPET
cd $HOME2_MIRROR
export CARGO_TARGET_DIR=$HOME2_TARGET XWIN_ACCEPT_LICENSE=1
set +e
out=\$(cargo xwin clippy --target x86_64-pc-windows-msvc -p archaeodash-desktop-app -- -D warnings 2>&1); rc=\$?
printf '%s\n' \"\$out\" | grep -E '^(warning|error)' -A6 | head -60
[ \$rc -eq 0 ] && echo ok
exit \$rc" ;;
  preview-log)
    run_remote '
if [ -f ~/build/preview.log ]; then tail -n 40 ~/build/preview.log; else echo "no preview log yet"; fi' ;;
esac
