# Build stage
FROM rust:1.90-slim as builder

WORKDIR /app

# Install system dependencies needed for building
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy dependency manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src/ ./src/

# Build the application in release mode
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false -m -d /app forwarding-proxy

WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/forwarding_translation_proxy /usr/local/bin/forwarding_translation_proxy

# Change ownership
RUN chown -R forwarding-proxy:forwarding-proxy /app

# Switch to app user
USER forwarding-proxy

ENTRYPOINT ["forwarding_translation_proxy"]