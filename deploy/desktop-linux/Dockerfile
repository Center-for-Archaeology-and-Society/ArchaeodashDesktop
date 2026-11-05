# Build environment for the Linux desktop app (AppImage and .deb).
# Ubuntu 22.04 keeps the glibc floor at 2.35, so the AppImage runs on
# Ubuntu 22.04+, Debian 12+ and similar. Used by scripts/home2/build-linux.sh;
# the build itself is scripts/build-linux-desktop.sh, run as the host user.
FROM ubuntu:22.04

ARG NODE_VERSION
ARG RUST_VERSION
ARG TAURI_CLI_VERSION=2.11.5

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential ca-certificates curl wget file xz-utils git pkg-config \
      libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
      librsvg2-dev libssl-dev libxdo-dev patchelf desktop-file-utils xdg-utils \
    && rm -rf /var/lib/apt/lists/*

# Node.js: the same version as on home2 (the web build reuses its node_modules).
RUN test -n "$NODE_VERSION" \
    && curl -fsSL "https://nodejs.org/dist/${NODE_VERSION}/node-${NODE_VERSION}-linux-x64.tar.xz" \
       | tar -xJ -C /usr/local --strip-components=1 \
    && node --version

# Rust: the toolchain pinned by rust-toolchain.toml, plus the Tauri CLI.
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN test -n "$RUST_VERSION" \
    && curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
       --default-toolchain "$RUST_VERSION" --component rustfmt --component clippy \
    && cargo install --locked tauri-cli --version "$TAURI_CLI_VERSION" \
    && rm -rf /opt/cargo/registry /opt/cargo/git \
    && chmod -R a+rwX /opt/rustup /opt/cargo
