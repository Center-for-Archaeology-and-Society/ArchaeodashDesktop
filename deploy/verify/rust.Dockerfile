# Rust toolchain image for the local CI-equivalent verification gate.
# The pinned toolchain (rust-toolchain.toml: 1.98.1 + rustfmt/clippy) is
# installed into the image so repeated runs only compile workspace crates.
FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libglib2.0-dev \
        libgtk-3-dev \
        libwebkit2gtk-4.1-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
    && rm -rf /var/lib/apt/lists/*

# Pre-install the exact pinned toolchain so first-run cache volumes start
# warm (rustup in the container honors rust-toolchain.toml regardless).
RUN rustup toolchain install 1.98.1 --profile minimal --component rustfmt,clippy \
    && rustup default 1.98.1
