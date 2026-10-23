#!/usr/bin/env bash
# Serves a production build of the web UI from home2 on port 4173 (for
# browser checks at http://192.168.0.37:4173/).
#
# Usage: bash scripts/home2/preview.sh start|stop|status
#   start   sync the mirror, build the web UI, (re)start the preview server
#   stop    stop the preview server
#   status  report whether it is serving
set -euo pipefail
. "$(dirname "$0")/common.sh"
action="${1:-}"
case "$action" in start|stop|status) ;; *) die "usage: bash scripts/home2/preview.sh start|stop|status";; esac
[ "$#" -eq 1 ] || die "usage: bash scripts/home2/preview.sh start|stop|status"

run_remote '
set -eo pipefail
action="$1"
stop() { pkill -f "[v]ite.js preview --host 0.0.0.0 --port 4173" || true; }
case "$action" in
  stop) stop; echo "preview stopped" ;;
  status)
    code=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:4173/ || true)
    if [ "$code" = 200 ]; then echo "preview serving at http://192.168.0.37:4173/"; else echo "preview not running"; fi ;;
  start)
    stop
    mkdir -p '"$HOME2_MIRROR"'
    '"$SYNC_SNIPPET"'
    cd '"$HOME2_MIRROR"'/apps/web
    node node_modules/vite/bin/vite.js build 2>&1 | grep -E "built in|error"
    nohup node node_modules/vite/bin/vite.js preview --host 0.0.0.0 --port 4173 --strictPort > ~/build/preview.log 2>&1 &
    for i in 1 2 3 4 5 6 7 8 9 10; do
      code=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:4173/ || true)
      [ "$code" = 200 ] && break
      sleep 1
    done
    echo "preview http ${code:-none} at http://192.168.0.37:4173/" ;;
esac
' "$action"
