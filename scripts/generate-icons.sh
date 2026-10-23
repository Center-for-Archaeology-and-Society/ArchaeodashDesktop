#!/usr/bin/env bash
# Regenerates the desktop app icons and the web UI brand assets from logo.png.
#
# Needs ImageMagick (6 `convert` or 7 `magick`) and python3 (for .icns).
# Usage: bash scripts/generate-icons.sh [path/to/logo.png]
#
# Two artworks are derived from the logo:
#   full - the whole logo (AD letters over the element table), for 64 px and up;
#   mark - the AD letters only, for 48 px and below, where the table's thin
#          lines and digits turn into noise. It is cut out by a morphological
#          opening of the alpha mask, which drops strokes thinner than ~18 px.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${1:-$ROOT/logo.png}"
ICONS="$ROOT/apps/desktop/src-tauri/icons"
PUBLIC="$ROOT/apps/web/public"
IM=convert
command -v magick >/dev/null 2>&1 && IM=magick
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$ICONS" "$PUBLIC"

# Fully transparent pixels in the source carry stray colour; clear it so it
# cannot bleed into resized edges.
CLEAN=(-background none -alpha background)
PNG=(-strip -define png:compression-level=9)

$IM "$SRC" "${CLEAN[@]}" -trim +repage "$WORK/full_trim.png"

$IM "$SRC" -alpha off "$WORK/rgb.png"
$IM "$SRC" -alpha extract -alpha off "$WORK/alpha.png"
$IM "$WORK/alpha.png" -threshold 50% -morphology Open Disk:9 -blur 0x1 -alpha off "$WORK/open.png"
$IM "$WORK/alpha.png" "$WORK/open.png" -compose Multiply -composite -alpha off "$WORK/mark_alpha.png"
$IM "$WORK/rgb.png" "$WORK/mark_alpha.png" -alpha off -compose CopyOpacity -composite \
  "${CLEAN[@]}" -trim +repage "$WORK/mark_trim.png"

# Pad to a square with a 4% margin on each side.
square() {
  local side
  side=$($IM "$1" -format '%[fx:ceil(max(w,h)*1.08)]' info:)
  $IM "$1" -gravity center -background none -extent "${side}x${side}" "$2"
}
square "$WORK/full_trim.png" "$WORK/full.png"
square "$WORK/mark_trim.png" "$WORK/mark.png"

# render ARTWORK SIZE OUT: square PNG; small sizes get a light sharpen.
render() {
  local sharpen=()
  if (($2 <= 48)); then sharpen=(-unsharp 0x0.6+0.6+0.01); fi
  $IM "$WORK/$1.png" -filter Lanczos -resize "$2x$2" ${sharpen[@]+"${sharpen[@]}"} \
    "${CLEAN[@]}" "${PNG[@]}" "PNG32:$3"
}
art() { if (($1 <= 48)); then echo mark; else echo full; fi; }

# --- Desktop (Tauri) icons ---------------------------------------------------
render mark 32 "$ICONS/32x32.png"
render full 64 "$ICONS/64x64.png"
render full 128 "$ICONS/128x128.png"
render full 256 "$ICONS/128x128@2x.png"
render full 512 "$ICONS/icon.png"

ico_parts=()
for size in 16 24 32 48 64 128 256; do
  render "$(art $size)" $size "$WORK/ico_$size.png"
  ico_parts+=("$WORK/ico_$size.png")
done
$IM "${ico_parts[@]}" "$ICONS/icon.ico"

# macOS .icns: PNG entries keyed by OSType. @2x entries for 16/32 pt keep the
# letters-only artwork because they are still displayed at 16/32 points.
render mark 16 "$WORK/icns_16.png"
render mark 32 "$WORK/icns_32.png"
render mark 64 "$WORK/icns_32x2.png"
render full 64 "$WORK/icns_64.png"
render full 128 "$WORK/icns_128.png"
render full 256 "$WORK/icns_256.png"
render full 512 "$WORK/icns_512.png"
render full 1024 "$WORK/icns_1024.png"
python3 - "$ICONS/icon.icns" \
  "icp4=$WORK/icns_16.png" "icp5=$WORK/icns_32.png" "icp6=$WORK/icns_64.png" \
  "ic07=$WORK/icns_128.png" "ic08=$WORK/icns_256.png" "ic09=$WORK/icns_512.png" \
  "ic10=$WORK/icns_1024.png" "ic11=$WORK/icns_32.png" "ic12=$WORK/icns_32x2.png" \
  "ic13=$WORK/icns_256.png" "ic14=$WORK/icns_512.png" <<'PY'
import struct, sys
chunks = b""
for entry in sys.argv[2:]:
    kind, path = entry.split("=", 1)
    data = open(path, "rb").read()
    chunks += kind.encode("ascii") + struct.pack(">I", len(data) + 8) + data
with open(sys.argv[1], "wb") as out:
    out.write(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)
PY

# --- Web UI assets (served from the site root by Vite) ------------------------
fav_parts=()
for size in 16 32 48; do fav_parts+=("$WORK/ico_$size.png"); done
$IM "${fav_parts[@]}" "$PUBLIC/favicon.ico"

# iOS draws home-screen icons on black unless they are opaque.
$IM "$WORK/full.png" -filter Lanczos -resize 160x160 -gravity center \
  -background '#f4fbf8' -extent 180x180 -flatten "${PNG[@]}" "$PUBLIC/apple-touch-icon.png"

render full 256 "$PUBLIC/logo-256.png"
render full 512 "$PUBLIC/logo-512.png"

# Header mark: 28 CSS px tall at 1x/2x/3x, natural aspect ratio.
for scale in 1 2 3; do
  height=$((28 * scale))
  suffix=""
  if ((scale > 1)); then suffix="@${scale}x"; fi
  sharpen=()
  if ((scale == 1)); then sharpen=(-unsharp 0x0.6+0.6+0.01); fi
  $IM "$WORK/mark_trim.png" -filter Lanczos -resize "x$height" ${sharpen[@]+"${sharpen[@]}"} \
    "${CLEAN[@]}" "${PNG[@]}" "PNG32:$PUBLIC/brand-mark$suffix.png"
done

echo "icons written to $ICONS and $PUBLIC"
$IM "$PUBLIC/brand-mark.png" -format 'brand-mark.png: %wx%h\n' info:
