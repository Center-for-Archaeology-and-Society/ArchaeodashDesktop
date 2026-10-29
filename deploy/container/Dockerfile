# syntax=docker/dockerfile:1
# Hosted ArchaeoDash image (Section 13.2: immutable deployment artifact;
# releases never mutate source on the host): the Rust `hosted` server plus
# the built web client, served by the same process. Build from the
# repository root:
#
#   docker build -f deploy/container/Dockerfile -t archaeodash-hosted .
#
# Dockerfile.dockerignore (next to this file) limits the build context.
# Runtime configuration is environment only; see deploy/compose/hosted-stack.yml.

# ---- Web client: vite build of apps/web (workspace packages are source) ----
FROM node:22-bookworm AS web
WORKDIR /src
ENV COREPACK_ENABLE_DOWNLOAD_PROMPT=0 \
    PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml tsconfig.base.json ./
COPY packages packages
COPY apps/web apps/web
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    corepack pnpm install --frozen-lockfile
RUN corepack pnpm --filter ./apps/web build

# ---- API: the hosted server binary ----
FROM rust:1-bookworm AS api
WORKDIR /src
# Same pin as rust-toolchain.toml, which cargo honours inside the build.
RUN rustup toolchain install 1.98.1 --profile minimal --component rustfmt,clippy
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
# Every workspace member's manifest must be present for cargo to load the
# workspace; only the API crate and its dependencies are compiled.
COPY crates crates
COPY apps/desktop/src-tauri apps/desktop/src-tauri
COPY tests/parity tests/parity
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p archaeodash-api --bin hosted \
    && cp target/release/hosted /usr/local/bin/hosted

# ---- Runtime: non-root, minimal ----
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
# The user file store (and each hosted project's workspace) lives under
# /data/user-files; a named volume mounted there inherits this ownership.
RUN useradd --system --uid 10001 --no-create-home archaeodash \
    && mkdir -p /data/user-files \
    && chown 10001:10001 /data/user-files
COPY --from=api /usr/local/bin/hosted /usr/local/bin/hosted
COPY --from=web /src/apps/web/dist /srv/archaeodash/web
COPY deploy/container/healthcheck /usr/local/bin/healthcheck
RUN chmod 755 /usr/local/bin/healthcheck
ENV AUTH_BIND=0.0.0.0:8080 \
    AUTH_WEB_DIR=/srv/archaeodash/web \
    AUTH_FILE_STORE_DIR=/data/user-files \
    AUTH_LOG_FORMAT=json
USER 10001
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=5s --start-period=20s --retries=3 \
    CMD ["/usr/local/bin/healthcheck"]
ENTRYPOINT ["/usr/local/bin/hosted"]
