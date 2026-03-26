# Build stage
FROM rust:1.94-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends cmake libclang-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy workspace manifests first for layer caching
COPY Cargo.toml Cargo.lock ./
COPY crates/matching-engine/Cargo.toml crates/matching-engine/
COPY crates/matching-solver/Cargo.toml crates/matching-solver/
COPY crates/matching-scenarios/Cargo.toml crates/matching-scenarios/
COPY crates/matching-sim/Cargo.toml crates/matching-sim/
COPY crates/matching-sequencer/Cargo.toml crates/matching-sequencer/
COPY crates/sybil-api/Cargo.toml crates/sybil-api/
COPY crates/sybil-oracle/Cargo.toml crates/sybil-oracle/
COPY crates/sybil-verifier/Cargo.toml crates/sybil-verifier/

# Create dummy source files to cache dependency compilation
RUN for crate in matching-engine matching-solver matching-scenarios matching-sim matching-sequencer sybil-api sybil-oracle sybil-verifier; do \
        mkdir -p crates/$crate/src && echo "" > crates/$crate/src/lib.rs; \
    done && \
    mkdir -p crates/sybil-api/src && echo "fn main() {}" > crates/sybil-api/src/main.rs && \
    mkdir -p crates/matching-sim/src && echo "fn main() {}" > crates/matching-sim/src/main.rs && \
    mkdir -p crates/matching-sequencer/src/bin && echo "fn main() {}" > crates/matching-sequencer/src/bin/sybil_sim.rs

# Build dependencies only (cached layer)
RUN cargo build --release -p sybil-api 2>/dev/null || true

# Copy actual source code
COPY crates/ crates/

# Build the actual binary
RUN cargo build --release -p sybil-api

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sybil-api /usr/local/bin/sybil-api

EXPOSE 3000

ENTRYPOINT ["sybil-api"]
