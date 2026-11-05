#!/usr/bin/env bash
# Publishes the last home2 desktop builds on cas, one folder per platform:
#   https://cas.rc.asu.edu/archaeodash/download/windows/ArchaeoDash.exe
#   https://cas.rc.asu.edu/archaeodash/download/linux/ArchaeoDash.AppImage
#   https://cas.rc.asu.edu/archaeodash/download/linux/ArchaeoDash.deb
# Each folder also gets versioned copies (ArchaeoDash-<short sha>.<ext>, the
# three newest kept) and SHA256SUMS. Files live in
# /var/www/archaeodash-downloads/<platform> (root-owned, world-readable).
# Builds come from build-windows.sh and build-linux.sh; the version label is
# the commit recorded in each build's BUILD_INFO.
#
# Also installs deploy/cas/apache-archaeodash.conf when it differs from the
# installed copy, but only if the two match outside the "BEGIN/END download"
# blocks (blank lines ignored), so hand edits on cas are never overwritten.
# The old file is backed up in /root, `apachectl configtest` must pass before
# the reload, and the backup is restored if it does not.
#
# Usage: bash scripts/home2/deploy-desktop-cas.sh [all|windows|linux]
set -euo pipefail
. "$(dirname "$0")/common.sh"
usage="usage: bash scripts/home2/deploy-desktop-cas.sh [all|windows|linux]"
[ "$#" -le 1 ] || die "$usage"
case "${1:-all}" in
  all) platforms=(windows linux) ;;
  windows|linux) platforms=("$1") ;;
  *) die "$usage" ;;
esac

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
CONF="$repo_root/deploy/cas/apache-archaeodash.conf"
CAS_SSH=(ssh -o BatchMode=yes -o ConnectTimeout=20 -o "ProxyCommand=ssh $HOME2_HOST ssh homepc -W %h:%p" cas)
BASE_URL=https://cas.rc.asu.edu/archaeodash/download

# Build files per platform on home2: "<path> <stable name>" lines.
files_for() {
  case "$1" in
    windows) echo "$HOME2_TARGET/x86_64-pc-windows-msvc/release/archaeodash-desktop.exe ArchaeoDash.exe" ;;
    linux)
      echo "$HOME2_TARGET_LINUX/release/bundle/appimage/ArchaeoDash_*_amd64.AppImage ArchaeoDash.AppImage"
      echo "$HOME2_TARGET_LINUX/release/bundle/deb/ArchaeoDash_*_amd64.deb ArchaeoDash.deb"
      ;;
  esac
}
info_for() {
  case "$1" in
    windows) echo "$HOME2_TARGET/BUILD_INFO" ;;
    linux) echo "$HOME2_TARGET_LINUX/BUILD_INFO" ;;
  esac
}

# Build paths hold "~" and globs meant for home2; never expand them here.
set -f
checks=()
for platform in "${platforms[@]}"; do
  echo "== $platform: build on home2"
  # Prints "commit=<sha>" then one "<stable name> <sha256> <size> <built>" line per file.
  info=$(run_remote '
set -euo pipefail
info="${1/#\~/$HOME}"; shift
[ -f "$info" ] || { echo "no BUILD_INFO next to the build; rebuild with scripts/home2/build-'"$platform"'.sh" >&2; exit 2; }
grep "^commit=" "$info"
dirty=$(sed -n "s/^dirty=//p" "$info")
[ "${dirty:-0}" = 0 ] || echo "note: built with $dirty uncommitted tracked change(s)" >&2
while [ "$#" -gt 0 ]; do
  pattern="${1/#\~/$HOME}"; matches=( $pattern ); name="$2"; shift 2
  [ "${#matches[@]}" -eq 1 ] && [ -f "${matches[0]}" ] || { echo "expected one build file for $name, found: ${matches[*]}" >&2; exit 2; }
  f="${matches[0]}"
  echo "$name $(sha256sum "$f" | cut -d" " -f1) $(stat -c %s "$f") $(date -u -r "$f" +%Y-%m-%dT%H:%MZ)"
done
' "$(info_for "$platform")" $(files_for "$platform"))
  SHA=$(printf '%s\n' "$info" | sed -n 's/^commit=//p')
  [[ "$SHA" =~ ^[0-9a-f]{7,12}$ ]] || die "unexpected build info: $info"
  printf '%s\n' "$info" | tail -n +2 | while read -r name sum size built; do
    [[ "$sum" =~ ^[0-9a-f]{64}$ && "$size" =~ ^[0-9]+$ ]] || die "unexpected file info: $name $sum $size"
    echo "commit $SHA: $name, built $built, $size bytes, sha256 $sum"
  done

  echo "== $platform: upload to cas"
  run_remote '
set -euo pipefail
platform="$1"; shift
i=0
while [ "$#" -gt 0 ]; do
  pattern="${1/#\~/$HOME}"; matches=( $pattern ); shift 2
  gzip -1 -c "${matches[0]}" | ssh homepc "ssh cas \"gunzip > /tmp/archaeodash-upload-$platform-$i\""
  i=$((i + 1))
done
' "$platform" $(files_for "$platform")

  # name sum per file, in upload order.
  "${CAS_SSH[@]}" bash -s -- "$platform" "$SHA" $(printf '%s\n' "$info" | tail -n +2 | awk '{print $1, $2}') <<'REMOTE'
set -euo pipefail
platform="$1"; sha="$2"; shift 2
dir="/var/www/archaeodash-downloads/$platform"
sudo install -d -m 755 -o root -g root /var/www/archaeodash-downloads "$dir"
i=0
while [ "$#" -gt 0 ]; do
  name="$1"; sum="$2"; shift 2
  up="/tmp/archaeodash-upload-$platform-$i"
  i=$((i + 1))
  [ "$(sha256sum "$up" | cut -d' ' -f1)" = "$sum" ] || { rm -f "$up"; echo "upload checksum mismatch for $name" >&2; exit 3; }
  ext="${name##*.}"
  sudo install -m 644 -o root -g root "$up" "$dir/ArchaeoDash-$sha.$ext"
  sudo install -m 644 -o root -g root "$up" "$dir/$name.tmp"
  sudo mv -f "$dir/$name.tmp" "$dir/$name"
  rm -f "$up"
  # Keep the three newest versioned builds of this type.
  ls -1t "$dir"/ArchaeoDash-*."$ext" | tail -n +4 | xargs -r sudo rm -f
done
(cd "$dir" && sha256sum ArchaeoDash.* ArchaeoDash-*) | sudo tee "$dir/SHA256SUMS" >/dev/null
sudo chmod 644 "$dir/SHA256SUMS"
# Files from before the per-platform folders (the old link redirects).
sudo rm -f /var/www/archaeodash-downloads/ArchaeoDash.exe /var/www/archaeodash-downloads/ArchaeoDash-*.exe /var/www/archaeodash-downloads/SHA256SUMS
ls -la "$dir"
REMOTE
  while read -r name sum size _; do
    checks+=("$platform/$name $sum $size")
  done < <(printf '%s\n' "$info" | tail -n +2)
done

echo "== apache"
"${CAS_SSH[@]}" "cat > /tmp/archaeodash-location.conf.new" < "$CONF"
"${CAS_SSH[@]}" bash -s <<'REMOTE'
set -euo pipefail
new=/tmp/archaeodash-location.conf.new
trap 'rm -f "$new"' EXIT
conf=/etc/apache2/conf-available/archaeodash-location.conf
strip() { sed -e '/^# BEGIN download/,/^# END download/d' -e '/^[[:space:]]*$/d'; }
if sudo cmp -s "$conf" "$new"; then
  echo "apache: config already current"
  exit 0
fi
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
REMOTE

echo "== public checks"
status=0
sleep 2
for c in "${checks[@]}"; do
  read -r path sum size <<<"$c"
  url="$BASE_URL/$path"
  hdr=$(curl -sI -m 30 "$url" || true)
  code=$(printf '%s' "$hdr" | head -1 | awk '{print $2}')
  len=$(printf '%s' "$hdr" | tr -d '\r' | awk -F': ' 'tolower($1)=="content-length"{print $2}')
  got=$(curl -s -m 900 "$url" | sha256sum | cut -d' ' -f1)
  if [ "$code" = 200 ] && [ "$len" = "$size" ] && [ "$got" = "$sum" ]; then
    echo "ok  $url ($len bytes, checksum matches)"
  else
    echo "BAD $url (status $code, $len bytes, sha256 $got)" >&2
    status=1
  fi
done
old=$(curl -s -o /dev/null -m 20 -w "%{http_code} %{redirect_url}" "$BASE_URL/ArchaeoDash.exe" || true)
echo "old link: $old"
[[ "$old" == "301 $BASE_URL/windows/ArchaeoDash.exe" ]] || status=1
for u in https://cas.rc.asu.edu/archaeodash/ https://cas.rc.asu.edu/archaeodash/healthz https://cas.rc.asu.edu/app/Archaeodash/; do
  c=$(curl -s -o /dev/null -m 20 -w "%{http_code}" "$u" || true)
  echo "$c $u"
  [ "$c" = 200 ] || status=1
done
[ $status -eq 0 ] && echo "published: ${platforms[*]}" || { echo "published, but a check failed" >&2; exit 1; }
