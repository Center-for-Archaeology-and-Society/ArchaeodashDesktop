#!/usr/bin/env bash
# Syncs the home2 mirror and builds the Linux desktop app (AppImage and .deb)
# in an Ubuntu 22.04 container (deploy/desktop-linux/Dockerfile), so the
# result runs on older distributions than home2's own. The container image
# is rebuilt only when the Dockerfile or the pinned versions change. Takes no
# arguments. Publish with deploy-desktop-cas.sh.
#
# Usage: bash scripts/home2/build-linux.sh
set -euo pipefail
. "$(dirname "$0")/common.sh"
[ "$#" -eq 0 ] || die "usage: bash scripts/home2/build-linux.sh"

run_remote "set -eo pipefail
mkdir -p $HOME2_MIRROR $HOME2_TARGET_LINUX ~/.cache/archaeodash-linux-build
$SYNC_SNIPPET
cd $HOME2_MIRROR
node_version=\$(node --version)
rust_version=\$(sed -n 's/^channel *= *\"\\(.*\\)\"/\\1/p' rust-toolchain.toml)
tag=\$(cat deploy/desktop-linux/Dockerfile | sha256sum | cut -c1-12)-\$node_version-\$rust_version
image=archaeodash-linux-build:\$tag
if ! docker image inspect \"\$image\" >/dev/null 2>&1; then
  echo \"building the build image \$image (first time takes a while)\"
  docker build -q --build-arg NODE_VERSION=\$node_version --build-arg RUST_VERSION=\$rust_version \
    -t \"\$image\" deploy/desktop-linux >/dev/null
fi
$BUILD_INFO_SNIPPET > $HOME2_TARGET_LINUX/BUILD_INFO.next
docker run --rm --user \$(id -u):\$(id -g) \
  -e HOME=/cache/home -e CARGO_HOME=/cache/cargo -e CARGO_TARGET_DIR=/target -e CARGO_BUILD_JOBS=2 \
  -v \"\$PWD\":/src -v $HOME2_TARGET_LINUX:/target -v ~/.cache/archaeodash-linux-build:/cache \
  -w /src \"\$image\" bash -c 'mkdir -p /cache/home /cache/cargo && bash scripts/build-linux-desktop.sh' 2>&1 \
  | grep -E 'built in|error|Finished|Bundling|Built ' | tail -12
mv -f $HOME2_TARGET_LINUX/BUILD_INFO.next $HOME2_TARGET_LINUX/BUILD_INFO
cat $HOME2_TARGET_LINUX/BUILD_INFO
ls -la --time-style=+%Y-%m-%dT%H:%M $HOME2_TARGET_LINUX/release/bundle/appimage/*.AppImage $HOME2_TARGET_LINUX/release/bundle/deb/*.deb"
