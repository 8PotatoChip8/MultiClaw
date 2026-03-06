# syntax=docker/dockerfile:1
FROM rust:1-slim-bookworm as builder

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

# Install runtime dependencies, incus client, and Docker CLI
# This container needs host network access in docker-compose
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    gnupg \
    git \
    && mkdir -p /etc/apt/keyrings/ \
    && curl -fsSL https://pkgs.zabbly.com/key.asc -o /etc/apt/keyrings/zabbly.asc \
    && echo "deb [signed-by=/etc/apt/keyrings/zabbly.asc] https://pkgs.zabbly.com/incus/stable bookworm main" > /etc/apt/sources.list.d/zabbly-incus-stable.list \
    && curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc \
    && chmod a+r /etc/apt/keyrings/docker.asc \
    && echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian bookworm stable" > /etc/apt/sources.list.d/docker.list \
    && apt-get update && apt-get install -y incus-client docker-ce-cli \
    && rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage
COPY --from=builder /usr/src/app/packages/target/release/control-plane /app/control-plane

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD curl -sf http://localhost:8080/v1/health || exit 1

CMD ["./control-plane"]
