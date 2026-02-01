# =============================================================================
# Titan Cloud API - Multi-stage Dockerfile
# =============================================================================
#
# This Dockerfile builds the cloud-api gRPC server.
#
# Build stages:
#   1. chef      - Install cargo-chef for dependency caching
#   2. planner   - Create dependency recipe
#   3. builder   - Build the application with cached dependencies
#   4. runtime   - Minimal runtime image
#
# Usage:
#   docker build -t titan-cloud-api -f apps/cloud-api/Dockerfile .
#   docker run -p 50051:50051 titan-cloud-api

# -----------------------------------------------------------------------------
# Stage 1: Chef - Base image with cargo-chef
# -----------------------------------------------------------------------------
FROM rust:1.87-slim-bookworm AS chef

# Install dependencies needed for compilation
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Install cargo-chef for dependency caching
RUN cargo install cargo-chef

WORKDIR /app

# -----------------------------------------------------------------------------
# Stage 2: Planner - Analyze dependencies
# -----------------------------------------------------------------------------
FROM chef AS planner

# Copy entire workspace (needed for workspace resolution)
COPY . .

# Create recipe.json (dependency lockfile)
RUN cargo chef prepare --recipe-path recipe.json

# -----------------------------------------------------------------------------
# Stage 3: Builder - Compile with cached dependencies
# -----------------------------------------------------------------------------
FROM chef AS builder

# Copy the recipe from planner
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies (this is cached unless dependencies change)
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source code
COPY . .

# Build the cloud-api binary
RUN cargo build --release --bin cloud-api

# -----------------------------------------------------------------------------
# Stage 4: Runtime - Minimal production image
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 -s /bin/bash titan

# Create app directory
WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/cloud-api /app/cloud-api

# Copy migrations (for runtime execution if needed)
COPY --from=builder /app/migrations/postgres /app/migrations

# Set ownership
RUN chown -R titan:titan /app

# Switch to non-root user
USER titan

# Expose gRPC port
EXPOSE 50051

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=3 \
    CMD nc -z localhost 50051 || exit 1

# Set environment defaults
ENV RUST_LOG=info,titan_cloud_api=debug
ENV GRPC_PORT=50051

# Run the server
CMD ["/app/cloud-api"]
