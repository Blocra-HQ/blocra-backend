# syntax=docker/dockerfile:1

# --- build ---
FROM rust:1.96-slim-bookworm AS builder
WORKDIR /app
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*

# Build dependencies first so they're cached across source changes.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
 && cargo build --release --locked \
 && rm -rf src

# migrations/ is embedded at compile time by sqlx::migrate!.
COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs && cargo build --release --locked

# --- runtime ---
FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home /app blocra
WORKDIR /app
COPY --from=builder /app/target/release/blocra-backend /app/blocra-backend

# SQLite (main and analytics databases) and uploaded images live on one
# volume so they survive redeploys; mount persistent storage at /app/data.
RUN mkdir -p /app/data/uploads && chown -R blocra:blocra /app
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
VOLUME ["/app/data"]
ENV HOST=0.0.0.0 \
    PORT=5000 \
    DATABASE_URL=sqlite:/app/data/blocra.db?mode=rwc \
    RUST_LOG=info

EXPOSE 5000
# Starts as root only to fix volume ownership, then drops to the blocra user
# (see docker-entrypoint.sh). JWT_SECRET must be provided at run time; the
# server refuses to start without it.
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
