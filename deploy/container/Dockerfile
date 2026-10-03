# Hosted API image (Section 13.2: immutable deployment artifact; releases
# never mutate source on the host). Multi-stage: cargo-chef layer caching,
# non-root runtime user, distroless-style minimal final stage.
FROM lukemathwalker/cargo-chef:latest-rust-1.82 AS chef
WORKDIR /app

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY IMPLEMENTATION.md .
RUN cargo build --release -p archaeodash-api --bin hosted

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/hosted /usr/local/bin/hosted
COPY deploy/container/healthcheck /usr/local/bin/healthcheck
RUN chmod +x /usr/local/bin/healthcheck \
    && useradd --system --uid 10001 --no-create-home archaeodash
USER 10001
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/hosted"]
