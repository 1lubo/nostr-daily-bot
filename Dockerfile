# ============================================================================
# Nostr Daily Bot - Multi-stage Dockerfile
# ============================================================================
# Build: docker build -t nostr-daily-bot .
# Run:   docker run -p 3000:3000 nostr-daily-bot
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

# Create dummy src and static to cache dependencies
RUN mkdir src && \
    mkdir static && \
    echo "fn main() {}" > src/main.rs && \
    echo "<html></html>" > static/index.html && \
    cargo build --release && \
    rm -rf src static

# Copy actual source code and static assets
COPY src ./src
COPY static ./static

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
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -u 1000 -s /bin/bash nostr

WORKDIR /app

# Create data directory for persistence
RUN mkdir -p /home/nostr/.config/nostr-daily-bot && \
    chown -R nostr:nostr /home/nostr/.config

# Copy binary from builder
COPY --from=builder /app/target/release/nostr-daily-bot /app/nostr-daily-bot

# Set ownership
RUN chown -R nostr:nostr /app

# Run as non-root user
USER nostr

# Environment variables
ENV RUST_LOG=info
ENV LOG_FORMAT=json

# Expose web UI port
EXPOSE 3000

# Health check via HTTP endpoint
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3000/api/status || exit 1

# Default command: start the web server
ENTRYPOINT ["/app/nostr-daily-bot"]
CMD ["serve", "--port", "3000"]
