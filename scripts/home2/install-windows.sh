#!/usr/bin/env bash
# Installs the last home2 Windows build on this PC:
# %LOCALAPPDATA%\Programs\ArchaeoDash\ArchaeoDash.exe, keeping the current
# one as ArchaeoDash.previous.exe. Refuses while the app is running.
#
# Usage: bash scripts/home2/install-windows.sh [--launch]
set -euo pipefail
. "$(dirname "$0")/common.sh"
launch=0
case "${1:-}" in
  "") ;;
  --launch) launch=1 ;;
  *) die "usage: bash scripts/home2/install-windows.sh [--launch]" ;;
esac
[ "$#" -le 1 ] || die "usage: bash scripts/home2/install-windows.sh [--launch]"
[ -n "${LOCALAPPDATA:-}" ] || die "LOCALAPPDATA is not set (run this on the Windows PC)"

if tasklist //FI "IMAGENAME eq ArchaeoDash.exe" 2>/dev/null | grep -qi "ArchaeoDash.exe"; then
  die "ArchaeoDash is running; close it first."
fi

dest="$LOCALAPPDATA/Programs/ArchaeoDash"
mkdir -p "$dest"
remote_exe="$HOME2_TARGET/x86_64-pc-windows-msvc/release/archaeodash-desktop.exe"
scp -q -o BatchMode=yes "$HOME2_HOST:${remote_exe#\~/}" "$dest/ArchaeoDash.new.exe"
[ -f "$dest/ArchaeoDash.exe" ] && cp -f "$dest/ArchaeoDash.exe" "$dest/ArchaeoDash.previous.exe"
mv -f "$dest/ArchaeoDash.new.exe" "$dest/ArchaeoDash.exe"
ls -la "$dest"

if [ "$launch" -eq 1 ]; then
  cmd //c start "" "$(cygpath -w "$dest/ArchaeoDash.exe")"
  sleep 8
  if tasklist //FI "IMAGENAME eq ArchaeoDash.exe" 2>/dev/null | grep -qi "ArchaeoDash.exe"; then
    echo "launched; recent log:"
  else
    echo "ArchaeoDash did not stay running; recent log:"
  fi
  log_dir="$LOCALAPPDATA/org.archaeodash.desktop/logs"
  latest="$(ls -t "$log_dir" 2>/dev/null | head -1)"
  [ -n "$latest" ] && tail -n 4 "$log_dir/$latest"
fi
