#!/usr/bin/env bash
# Publishes the last home2 Windows desktop build (build-windows.sh) on cas:
#   https://cas.rc.asu.edu/archaeodash/download/ArchaeoDash.exe
# plus ArchaeoDash-<short sha>.exe and SHA256SUMS. Files live in
# /var/www/archaeodash-downloads (root-owned, world-readable; the last three
# versioned builds are kept).
#
# Also installs deploy/cas/apache-archaeodash.conf when it differs from the
# installed copy, but only if the two match outside the "BEGIN/END download"
# blocks (so hand edits on cas are never overwritten). The old file is backed
# up in /root, `apachectl configtest` must pass before the reload, and the
# backup is restored if it does not.
#
# Takes no arguments, so allowing it allows nothing else.
#
# Usage: bash scripts/home2/deploy-desktop-cas.sh
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -eq 0 ] || die "usage: bash scripts/home2/deploy-desktop-cas.sh"

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
CONF="$repo_root/deploy/cas/apache-archaeodash.conf"
CAS_SSH=(ssh -o BatchMode=yes -o ConnectTimeout=20 -o "ProxyCommand=ssh $HOME2_HOST ssh homepc -W %h:%p" cas)
URL=https://cas.rc.asu.edu/archaeodash/download/ArchaeoDash.exe

echo "== build on home2"
info=$(run_remote '
set -euo pipefail
exe='"$HOME2_TARGET"'/x86_64-pc-windows-msvc/release/archaeodash-desktop.exe
[ -f "$exe" ] || { echo "no Windows build; run scripts/home2/build-windows.sh" >&2; exit 2; }
sha=$(git -C '"$HOME2_REPO"' -c safe.directory="*" rev-parse --short HEAD)
echo "$sha $(sha256sum "$exe" | cut -d" " -f1) $(stat -c %s "$exe") $(date -u -r "$exe" +%Y-%m-%dT%H:%MZ)"
')
read -r SHA SUM SIZE BUILT <<<"$info"
[[ "$SHA" =~ ^[0-9a-f]{7,12}$ && "$SUM" =~ ^[0-9a-f]{64}$ && "$SIZE" =~ ^[0-9]+$ ]] || die "unexpected build info: $info"
echo "commit $SHA, built $BUILT, $SIZE bytes, sha256 $SUM"

echo "== upload to cas"
run_remote '
set -euo pipefail
exe='"$HOME2_TARGET"'/x86_64-pc-windows-msvc/release/archaeodash-desktop.exe
gzip -1 -c "$exe" | ssh homepc "ssh cas \"gunzip > /tmp/archaeodash-desktop-upload.exe\""
'
"${CAS_SSH[@]}" "cat > /tmp/archaeodash-location.conf.new" < "$CONF"

"${CAS_SSH[@]}" bash -s -- "$SHA" "$SUM" <<'REMOTE'
set -euo pipefail
sha="$1"; sum="$2"
up=/tmp/archaeodash-desktop-upload.exe
new=/tmp/archaeodash-location.conf.new
trap 'rm -f "$up" "$new"' EXIT
[ "$(sha256sum "$up" | cut -d' ' -f1)" = "$sum" ] || { echo "upload checksum mismatch" >&2; exit 3; }
dir=/var/www/archaeodash-downloads
sudo install -d -m 755 -o root -g root "$dir"
name="ArchaeoDash-$sha.exe"
sudo install -m 644 -o root -g root "$up" "$dir/$name"
sudo install -m 644 -o root -g root "$up" "$dir/ArchaeoDash.exe.tmp"
sudo mv -f "$dir/ArchaeoDash.exe.tmp" "$dir/ArchaeoDash.exe"
# Keep the three newest versioned builds.
ls -1t "$dir"/ArchaeoDash-*.exe | tail -n +4 | xargs -r sudo rm -f
(cd "$dir" && sha256sum ArchaeoDash.exe ArchaeoDash-*.exe) | sudo tee "$dir/SHA256SUMS" >/dev/null
sudo chmod 644 "$dir/SHA256SUMS"
ls -la "$dir"

conf=/etc/apache2/conf-available/archaeodash-location.conf
strip() { sed '/^# BEGIN download/,/^# END download/d'; }
if sudo cmp -s "$conf" "$new"; then
  echo "apache: config already current"
else
  if ! diff <(sudo cat "$conf" | strip) <(strip < "$new") >/dev/null; then
    echo "apache: the installed config differs from the repo outside the download blocks; not replacing it" >&2
    sudo cat "$conf" | strip | diff - <(strip < "$new") >&2 || true
    exit 4
  fi
  backup="/root/archaeodash-location.conf.bak-$(date +%Y%m%d%H%M%S)"
  sudo cp -p "$conf" "$backup"
  sudo install -m 644 -o root -g root "$new" "$conf"
  if test_out=$(sudo apachectl configtest 2>&1); then
    printf '%s\n' "$test_out" | tail -1
    sudo systemctl reload apache2
    echo "apache: config installed and reloaded (backup $backup)"
  else
    printf '%s\n' "$test_out" >&2
    sudo cp -p "$backup" "$conf"
    echo "apache: configtest failed; restored $backup, not reloaded" >&2
    exit 5
  fi
fi
REMOTE

echo "== public checks"
status=0
sleep 2
hdr=$(curl -sI -m 30 "$URL" || true)
code=$(printf '%s' "$hdr" | head -1 | awk '{print $2}')
len=$(printf '%s' "$hdr" | tr -d '\r' | awk -F': ' 'tolower($1)=="content-length"{print $2}')
echo "$code $URL ($len bytes)"
[ "$code" = 200 ] && [ "$len" = "$SIZE" ] || status=1
got=$(curl -s -m 600 "$URL" | sha256sum | cut -d' ' -f1)
[ "$got" = "$SUM" ] && echo "download checksum ok" || { echo "download checksum mismatch: $got" >&2; status=1; }
for u in https://cas.rc.asu.edu/archaeodash/ https://cas.rc.asu.edu/archaeodash/healthz https://cas.rc.asu.edu/app/Archaeodash/; do
  c=$(curl -s -o /dev/null -m 20 -w "%{http_code}" "$u" || true)
  echo "$c $u"
  [ "$c" = 200 ] || status=1
done
[ $status -eq 0 ] && echo "published ArchaeoDash-$SHA.exe" || { echo "published, but a check failed" >&2; exit 1; }
