# Titan POS - Docker & Cloud Deployment Guide

> **Version**: 0.1.0  
> **Last Updated**: January 31, 2026

---

## Overview

Titan POS consists of two main deployable components:

1. **Desktop App** (Tauri): Runs locally on POS terminals - NOT containerized
2. **Cloud Services** (Rust): Sync API, background workers - CONTAINERIZED

For v0.1 (Logical Core), we focus on:
- Docker setup for development environment
- PostgreSQL for cloud database
- Preparation for future cloud services

---

## Local Development with Docker

### docker-compose.yml (Development)

```yaml
version: '3.8'

services:
  # PostgreSQL for cloud database (mimics production)
  postgres:
    image: postgres:16-alpine
    container_name: titan-postgres
    environment:
      POSTGRES_USER: titan
      POSTGRES_PASSWORD: titan_dev_password
      POSTGRES_DB: titan_pos
    ports:
      - "5432:5432"
    volumes:
      - postgres_data:/var/lib/postgresql/data
      - ./migrations/postgres:/docker-entrypoint-initdb.d
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U titan -d titan_pos"]
      interval: 10s
      timeout: 5s
      retries: 5

  # Redis for caching and pub/sub (future use)
  redis:
    image: redis:7-alpine
    container_name: titan-redis
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 10s
      timeout: 5s
      retries: 5

  # Adminer for database management (dev only)
  adminer:
    image: adminer:latest
    container_name: titan-adminer
    ports:
      - "8080:8080"
    depends_on:
      - postgres

volumes:
  postgres_data:
  redis_data:

networks:
  default:
    name: titan-network
```

### Usage

```bash
# Start all services
docker compose up -d

# View logs
docker compose logs -f postgres

# Stop all services
docker compose down

# Reset database (destructive)
docker compose down -v
docker compose up -d
```

---

## Cloud Services Architecture (Future: v1.0+)

### Dockerfile.cloud-api

```dockerfile
# Build stage
FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static

WORKDIR /app

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
COPY apps/cloud/ ./apps/cloud/

# Build release binary
RUN cargo build --release --package titan-cloud-api

# Runtime stage
FROM alpine:3.19

RUN apk add --no-cache ca-certificates

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/titan-cloud-api /app/titan-cloud-api

# Copy migrations
COPY migrations/postgres/ /app/migrations/

# Non-root user
RUN addgroup -g 1000 titan && \
    adduser -D -s /bin/sh -u 1000 -G titan titan && \
    chown -R titan:titan /app

USER titan

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://localhost:8000/health || exit 1

ENTRYPOINT ["/app/titan-cloud-api"]
```

### docker-compose.production.yml (Future)

```yaml
version: '3.8'

services:
  api:
    image: titan-pos/cloud-api:${VERSION:-latest}
    deploy:
      replicas: 3
      resources:
        limits:
          cpus: '1'
          memory: 512M
    environment:
      DATABASE_URL: ${DATABASE_URL}
      REDIS_URL: ${REDIS_URL}
      JWT_SECRET: ${JWT_SECRET}
    ports:
      - "8000:8000"
    healthcheck:
      test: ["CMD", "wget", "--spider", "http://localhost:8000/health"]
      interval: 30s
      timeout: 10s
      retries: 3

  sync-worker:
    image: titan-pos/sync-worker:${VERSION:-latest}
    deploy:
      replicas: 2
    environment:
      DATABASE_URL: ${DATABASE_URL}
      REDIS_URL: ${REDIS_URL}

  # Background job processor
  jobs-worker:
    image: titan-pos/jobs-worker:${VERSION:-latest}
    deploy:
      replicas: 1
    environment:
      DATABASE_URL: ${DATABASE_URL}
      REDIS_URL: ${REDIS_URL}
```

---

## Environment Variables

### Development (.env.development)

```bash
# Database
DATABASE_URL=postgres://titan:titan_dev_password@localhost:5432/titan_pos
SQLITE_PATH=./data/titan.db

# Redis
REDIS_URL=redis://localhost:6379

# Auth
JWT_SECRET=dev-secret-change-in-production
JWT_EXPIRY_SECONDS=3600

# Logging
RUST_LOG=debug,sqlx=warn
LOG_FORMAT=pretty

# Feature Flags
ENABLE_TELEMETRY=false
ENABLE_SYNC=false
```

### Production (.env.production)

```bash
# Database (use secrets manager in real deployment)
DATABASE_URL=${DATABASE_URL}
SQLITE_PATH=/app/data/titan.db

# Redis
REDIS_URL=${REDIS_URL}

# Auth
JWT_SECRET=${JWT_SECRET}
JWT_EXPIRY_SECONDS=900

# Logging
RUST_LOG=info,sqlx=warn
LOG_FORMAT=json

# Feature Flags
ENABLE_TELEMETRY=true
ENABLE_SYNC=true
```

---

## CI/CD Pipeline

### GitHub Actions Workflow

```yaml
# .github/workflows/ci.yml
name: CI

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets -- -D warnings

  test:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16-alpine
        env:
          POSTGRES_USER: titan
          POSTGRES_PASSWORD: titan_test
          POSTGRES_DB: titan_test
        ports:
          - 5432:5432
        options: >-
          --health-cmd pg_isready
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --all
        env:
          DATABASE_URL: postgres://titan:titan_test@localhost:5432/titan_test

  build-desktop:
    strategy:
      matrix:
        include:
          - os: macos-latest
            target: aarch64-apple-darwin
          - os: windows-latest
            target: x86_64-pc-windows-msvc
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: pnpm/action-setup@v2
        with:
          version: 8
      - uses: actions/setup-node@v4
        with:
          node-version: 20
          cache: pnpm
      - run: pnpm install
      - run: pnpm tauri build
      - uses: actions/upload-artifact@v4
        with:
          name: desktop-${{ matrix.target }}
          path: apps/desktop/src-tauri/target/release/bundle/
```

---

## Kubernetes Deployment (Future Reference)

### Namespace & ConfigMap

```yaml
# k8s/namespace.yml
apiVersion: v1
kind: Namespace
metadata:
  name: titan-pos

---
# k8s/configmap.yml
apiVersion: v1
kind: ConfigMap
metadata:
  name: titan-config
  namespace: titan-pos
data:
  RUST_LOG: "info,sqlx=warn"
  LOG_FORMAT: "json"
  ENABLE_TELEMETRY: "true"
```

### Deployment

```yaml
# k8s/deployment.yml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: titan-api
  namespace: titan-pos
spec:
  replicas: 3
  selector:
    matchLabels:
      app: titan-api
  template:
    metadata:
      labels:
        app: titan-api
    spec:
      containers:
        - name: api
          image: titan-pos/cloud-api:latest
          ports:
            - containerPort: 8000
          env:
            - name: DATABASE_URL
              valueFrom:
                secretKeyRef:
                  name: titan-secrets
                  key: database-url
          resources:
            requests:
              cpu: 100m
              memory: 128Mi
            limits:
              cpu: 500m
              memory: 512Mi
          livenessProbe:
            httpGet:
              path: /health
              port: 8000
            initialDelaySeconds: 10
            periodSeconds: 30
          readinessProbe:
            httpGet:
              path: /ready
              port: 8000
            initialDelaySeconds: 5
            periodSeconds: 10
```

---

## Development Workflow

### First-Time Setup

```bash
# 1. Clone repository
git clone https://github.com/your-org/titan-pos.git
cd titan-pos

# 2. Start Docker services
docker compose up -d

# 3. Wait for PostgreSQL to be ready
docker compose logs -f postgres
# Look for: "database system is ready to accept connections"

# 4. Install dependencies
pnpm install

# 5. Run database migrations
cargo run -p titan-db --bin migrate

# 6. Seed development data
cargo run -p titan-db --bin seed

# 7. Start development server
pnpm dev
```

### Daily Development

```bash
# Start services (if not running)
docker compose up -d

# Start dev server
pnpm dev

# Run tests
cargo test
pnpm test

# Format & lint
cargo fmt && cargo clippy
pnpm lint
```

---

## Monitoring & Observability (Future)

### Metrics (Prometheus)

```rust
// Health endpoint
async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_seconds": UPTIME.elapsed().as_secs()
    }))
}

// Metrics endpoint (Prometheus format)
async fn metrics() -> impl IntoResponse {
    // Export Prometheus metrics
}
```

### Logging (Structured JSON)

```rust
use tracing_subscriber::{fmt, EnvFilter};

fn init_logging() {
    let format = fmt::format()
        .json()
        .with_target(true)
        .with_level(true)
        .with_thread_ids(true);

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .event_format(format)
        .init();
}
```

---

*This guide covers containerization strategy for Titan POS*
