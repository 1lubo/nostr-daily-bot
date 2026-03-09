# ============================================================================
# Nostr Daily Bot - Multi-stage Dockerfile
# ============================================================================
# Build: docker build -t nostr-daily-bot .
# Run:   docker run -v ./config.toml:/app/config.toml -e NOSTR_PRIVATE_KEY=nsec1... nostr-daily-bot
# ============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Build
# -----------------------------------------------------------------------------
FROM rust:1.85-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Create dummy src to cache dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy actual source code
COPY src ./src

# Build the real application (touch to invalidate cache)
RUN touch src/main.rs && cargo build --release

# -----------------------------------------------------------------------------
# Stage 2: Runtime
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies (SSL for WebSocket connections)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 -s /bin/bash nostr

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/nostr-daily-bot /app/nostr-daily-bot

# Copy default config (can be overridden with volume mount)
COPY config.toml /app/config.toml

# Set ownership
RUN chown -R nostr:nostr /app

# Run as non-root user
USER nostr

# Environment variables
ENV RUST_LOG=info
ENV LOG_FORMAT=json

# Health check - process is running
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD pgrep -x nostr-daily-bot || exit 1

ENTRYPOINT ["/app/nostr-daily-bot"]

