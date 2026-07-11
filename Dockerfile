FROM rust:alpine AS chef
RUN apk add --no-cache build-base perl
RUN cargo install cargo-chef --locked

FROM chef AS planner
WORKDIR /app
# Only manifests — no source, so the recipe stays stable across code changes.
COPY Cargo.toml Cargo.lock ./
# cargo-chef needs the crate root to exist to resolve the manifest.
RUN mkdir src && touch src/main.rs
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /app
COPY --from=planner /app/recipe.json recipe.json
# Cook dependencies only — this layer is cached as long as Cargo.toml / Cargo.lock
# are unchanged, regardless of source edits.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo chef cook --release --bin discord-logging --recipe-path recipe.json
# Full source comes in here; only user code is recompiled from this point.
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin discord-logging && \
    cp target/release/discord-logging /discord-logging

FROM scratch
USER 1000:1000
WORKDIR /app
COPY --from=builder --chown=1000:1000 /discord-logging /app/discord-logging
ENTRYPOINT ["/app/discord-logging"]
