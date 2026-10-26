# Stage 1: Build Rust binary
FROM rust:1-bookworm AS builder

WORKDIR /build

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY web/viewer.legacy.html web/floorplan.legacy.html web/

# Build release binary (only bml-cli)
RUN cargo build --release -p bml-cli

# Stage 2: Build web frontend
FROM node:22-bookworm-slim AS web-builder

WORKDIR /build/web

COPY web/package.json web/package-lock.json ./
RUN npm ci

COPY web/ ./
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/buildingml /usr/local/bin/buildingml
COPY --from=web-builder /build/web/dist /app/web/dist
COPY tests/fixtures/ /app/examples/

EXPOSE 3001

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3001/health || exit 1

CMD ["buildingml", "serve", "--web-dir", "/app/web/dist", "--examples-dir", "/app/examples", "--port", "3001"]
