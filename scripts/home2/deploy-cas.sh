#!/usr/bin/env bash
# Deploys the committed HEAD to the hosted site on cas.
#
# Usage: bash scripts/home2/deploy-cas.sh
#
# Builds the hosted image (archaeodash-hosted:cas-<short sha>) on home2 from
# the repository there, ships it to cas through homepc (cas is reachable
# only from there), sets ARCHAEODASH_TAG in /srv/archaeodash/.env, restarts
# the app container, waits for it to be healthy and checks the public URLs.
# Takes no arguments, so allowing it allows nothing else. Uncommitted
# changes are not deployed: the build uses `git archive HEAD`.
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -eq 0 ] || die "usage: bash scripts/home2/deploy-cas.sh"

CAS_SSH=(ssh -o BatchMode=yes -o ConnectTimeout=20 -o "ProxyCommand=ssh $HOME2_HOST ssh homepc -W %h:%p" cas)

echo "== build on home2"
TAG=$(run_remote '
set -euo pipefail
cd '"$HOME2_REPO"'
g() { git -c safe.directory="*" "$@"; }
sha=$(g rev-parse --short HEAD)
tag="cas-$sha"
g log --oneline -1 | cat >&2
dirty=$(g status --short --untracked-files=no | wc -l)
[ "$dirty" -eq 0 ] || echo "note: $dirty uncommitted tracked change(s) are not deployed" >&2
build=$(mktemp -d)
trap "rm -rf $build" EXIT
g archive HEAD | tar -x -C "$build"
docker build -q -f "$build/deploy/container/Dockerfile" --build-arg ARCHAEODASH_BASE_PATH=/archaeodash \
  -t "archaeodash-hosted:$tag" "$build" >&2
echo "$tag"
')
echo "built $TAG"

echo "== ship to cas"
run_remote '
set -euo pipefail
docker save "archaeodash-hosted:$1" | gzip -1 | ssh homepc "ssh cas \"gunzip | docker load\""
' "$TAG"

echo "== switch the app on cas"
"${CAS_SSH[@]}" "set -e
sudo sed -i 's/^ARCHAEODASH_TAG=.*/ARCHAEODASH_TAG=$TAG/' /srv/archaeodash/.env
DC='sudo docker compose --project-directory /srv/archaeodash -f /srv/archaeodash/compose.yml'
\$DC up -d app 2>&1 | tail -2
for i in \$(seq 1 45); do [ \"\$(sudo docker inspect -f '{{.State.Health.Status}}' archaeodash-app-1)\" = healthy ] && break; sleep 2; done
\$DC ps --format '{{.Service}} {{.Image}} {{.Status}}'
curl -s -w ' healthz %{http_code}\n' http://127.0.0.1:18787/healthz"

echo "== public checks"
status=0
for u in https://cas.rc.asu.edu/archaeodash/ https://cas.rc.asu.edu/archaeodash/probabilities \
  https://cas.rc.asu.edu/archaeodash/healthz https://cas.rc.asu.edu/app/Archaeodash/; do
  code=$(curl -s -o /dev/null -m 20 -w "%{http_code}" "$u" || true)
  echo "$code $u"
  [ "$code" = 200 ] || status=1
done
[ $status -eq 0 ] && echo "deployed $TAG" || echo "deployed $TAG, but a public check failed"
exit $status
