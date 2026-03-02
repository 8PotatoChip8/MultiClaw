# syntax=docker/dockerfile:1
FROM rust:1.78-slim as builder

WORKDIR /usr/src/app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the entire workspace
COPY packages/ ./packages/

# Build the ollama-proxy package
RUN cargo build --release -p ollama-proxy

# Runner stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/target/release/ollama-proxy /app/ollama-proxy

EXPOSE 11436

CMD ["./ollama-proxy"]
