# Shared settings for the scripts/home2 wrappers (sourced, not run).
#
# The repository lives on home2 (192.168.0.37) and is mounted on this
# Windows PC as P:. home2 has the Rust/Node toolchains; the share is noexec
# there, so builds that run native tools use a local mirror on home2.
#
# These wrappers are the only home2 commands allowed without a prompt
# (.claude/settings.local.json). Each one runs a fixed command and accepts
# only validated arguments, so allowing them never allows arbitrary code on
# home2.

HOME2_HOST="home2"
HOME2_REPO="/mnt/pooled/Projects/Archaeodashdesktop"
HOME2_MIRROR="~/build/archaeodashdesktop"
HOME2_TARGET="~/archaeodash-target"

# run_remote SCRIPT_TEXT [ARGS...]: runs a bash script on home2 with
# positional arguments, quoting them so spaces and metacharacters stay data.
run_remote() {
  local script="$1"
  shift
  local quoted=""
  if [ "$#" -gt 0 ]; then
    quoted="$(printf '%q ' "$@")"
  fi
  printf '%s\n' "$script" | ssh -o BatchMode=yes -o ConnectTimeout=15 "$HOME2_HOST" "bash -s -- $quoted"
}

# Rsyncs the share into the home2 mirror (where native build tools can run).
SYNC_SNIPPET='rsync -a --delete --exclude target --exclude .git --exclude apps/web/dist \
  --exclude "node_modules/.pnpm-*" "'"$HOME2_REPO"'/" '"$HOME2_MIRROR"'/'

die() {
  printf '%s\n' "$*" >&2
  exit 2
}
