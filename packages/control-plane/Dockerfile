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

# Build the control-plane package
WORKDIR /usr/src/app/packages
RUN cargo build --release -p control-plane

# Runner stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies and incus client
# This container needs host network access in docker-compose
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    gnupg \
    && mkdir -p /etc/apt/keyrings/ \
    && curl -fsSL https://pkgs.zabbly.com/key.asc -o /etc/apt/keyrings/zabbly.asc \
    && echo "deb [signed-by=/etc/apt/keyrings/zabbly.asc] https://pkgs.zabbly.com/incus/stable bookworm main" > /etc/apt/sources.list.d/zabbly-incus-stable.list \
    && apt-get update && apt-get install -y incus-client \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/packages/target/release/control-plane /app/control-plane

EXPOSE 8080

CMD ["./control-plane"]
