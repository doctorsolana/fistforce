# Build stage - compile the Rust server
FROM rust:1.83-slim-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY server ./server

# Create dummy client to satisfy workspace (Cargo.toml + empty main.rs)
RUN mkdir -p client/src && \
    echo '[package]' > client/Cargo.toml && \
    echo 'name = "client"' >> client/Cargo.toml && \
    echo 'version = "0.1.0"' >> client/Cargo.toml && \
    echo 'edition = "2021"' >> client/Cargo.toml && \
    echo 'fn main() {}' > client/src/main.rs

# Build release binary (server only)
RUN cargo build --release --package server

# Runtime stage - minimal image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy the built binary
COPY --from=builder /app/target/release/server /usr/local/bin/server

# Expose UDP port for game traffic
EXPOSE 5000/udp

# Run the server
CMD ["server"]
