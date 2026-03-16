# ── Stage 1: Build React frontend ─────────────────────────────────────────────
FROM node:20-alpine AS frontend

WORKDIR /frontend

# Install deps first for better layer caching.
COPY frontend/package.json ./
RUN npm install

# Build the app.
COPY frontend/ ./
RUN npm run build
# Produces: /frontend/dist/

# ── Stage 2: Build Rust server ────────────────────────────────────────────────
# The agent/ directory is NOT a workspace member; no Windows deps present.

FROM rust:latest AS builder

WORKDIR /workspace

# Cache Cargo dependencies (invalidated only when manifests change).
COPY Cargo.toml Cargo.lock ./
COPY server/Cargo.toml server/Cargo.toml

RUN mkdir -p server/src \
 && echo 'fn main(){}' > server/src/main.rs \
 && cargo build --release --package server \
    2>/dev/null || true \
 && rm -rf server/src

# Real build – touch sources so Cargo rebuilds after the stub.
COPY server/ server/
RUN find server/src -name '*.rs' -exec touch {} + \
 && cargo build --release --package server

# ── Stage 3: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder   /workspace/target/release/server /app/server
# Serve the React SPA; migrations are embedded via sqlx::migrate!()
COPY --from=frontend  /frontend/dist                   /app/static

ENV STATIC_DIR=/app/static \
    RUST_LOG=info

EXPOSE 9000

ENTRYPOINT ["/app/server"]
