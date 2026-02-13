# Multi-stage Dockerfile for Shadow-Index
# Uses cargo-chef for optimal caching of Reth's massive dependency tree

# Stage 1: Chef - Install cargo-chef
FROM rust:1.85-bookworm AS chef
RUN cargo install cargo-chef
WORKDIR /app

# Stage 2: Planner - Generate recipe.json
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# Stage 3: Builder - Build dependencies then application
FROM rust:1.85-bookworm AS builder

# Install crucial OS dependencies for reth-db (MDBX) and other native bindings
RUN apt-get update && apt-get install -y \
    clang \
    libclang-dev \
    pkg-config \
    libssl-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef
RUN cargo install cargo-chef

WORKDIR /app

# Copy recipe and build dependencies (this layer is heavily cached)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source code and build the binary
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src ./src
RUN cargo build --release --bin shadow-index

# Stage 4: Runtime - Minimal image with only the binary
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled binary from builder
COPY --from=builder /app/target/release/shadow-index /usr/local/bin/shadow-index

# Expose ports
# 30303: P2P (Ethereum network)
# 8545: RPC (JSON-RPC API)
# 9001: Metrics (Prometheus)
EXPOSE 30303 8545 9001

# Set the entrypoint
ENTRYPOINT ["/usr/local/bin/shadow-index"]
