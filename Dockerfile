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

# SQLite and uploaded images live on volumes so they survive redeploys.
RUN mkdir -p /app/data /app/uploads && chown -R blocra /app/data /app/uploads
VOLUME ["/app/data", "/app/uploads"]
ENV HOST=0.0.0.0 \
    PORT=5000 \
    DATABASE_URL=sqlite:/app/data/blocra.db \
    RUST_LOG=info
USER blocra

EXPOSE 5000
# JWT_SECRET must be provided at run time; the server refuses to start without it.
CMD ["/app/blocra-backend"]
