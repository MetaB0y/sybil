# Build stage
FROM rust:1.94-bookworm AS builder

# Default to server-safe Rust build settings for the prod Linode
# (1 vCPU / 2 GB RAM / 495 MB swap). Local compose builds can override these
# through Docker build args without making remote builds more memory-hungry.
ARG CARGO_BUILD_JOBS=1
ARG CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
ENV CARGO_PROFILE_RELEASE_CODEGEN_UNITS=${CARGO_PROFILE_RELEASE_CODEGEN_UNITS}

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
COPY crates/sybil-api-types/Cargo.toml crates/sybil-api-types/
COPY crates/sybil-oracle/Cargo.toml crates/sybil-oracle/
COPY crates/sybil-verifier/Cargo.toml crates/sybil-verifier/
COPY crates/sybil-zk/Cargo.toml crates/sybil-zk/
COPY crates/sybil-witgen/Cargo.toml crates/sybil-witgen/
COPY crates/sybil-witgen-cli/Cargo.toml crates/sybil-witgen-cli/
COPY crates/sybil-prover/Cargo.toml crates/sybil-prover/
COPY crates/sybil-canonical/Cargo.toml crates/sybil-canonical/
COPY crates/sybil-polymarket/Cargo.toml crates/sybil-polymarket/

# Create dummy source files to cache dependency compilation
RUN for crate in matching-engine matching-solver matching-scenarios matching-sim matching-sequencer sybil-api sybil-api-types sybil-oracle sybil-verifier sybil-zk sybil-witgen sybil-canonical sybil-polymarket; do \
        mkdir -p crates/$crate/src && echo "" > crates/$crate/src/lib.rs; \
    done && \
    mkdir -p crates/sybil-api/src && echo "fn main() {}" > crates/sybil-api/src/main.rs && \
    mkdir -p crates/sybil-api/src/bin && echo "fn main() {}" > crates/sybil-api/src/bin/sybil_admin.rs && \
    mkdir -p crates/sybil-prover/src && echo "fn main() {}" > crates/sybil-prover/src/main.rs && \
    mkdir -p crates/sybil-witgen-cli/src && echo "fn main() {}" > crates/sybil-witgen-cli/src/main.rs && \
    mkdir -p crates/matching-sim/src && echo "fn main() {}" > crates/matching-sim/src/main.rs && \
    mkdir -p crates/matching-sequencer/src/bin && echo "fn main() {}" > crates/matching-sequencer/src/bin/sybil_sim.rs && \
    mkdir -p crates/matching-solver/benches && echo "fn main() {}" > crates/matching-solver/benches/solver_bench.rs && \
    mkdir -p crates/sybil-polymarket/src && echo "fn main() {}" > crates/sybil-polymarket/src/main.rs

# Build dependencies only (cached layer)
RUN cargo build --release -p sybil-api -p sybil-polymarket -p sybil-prover

# Copy actual source code
COPY crates/ crates/

# Remove artifacts produced from dummy workspace sources while preserving the
# third-party dependency cache built above. Cargo fingerprints can otherwise
# treat the dummy package binaries as fresh after COPY, yielding no-op services.
RUN cargo clean \
        -p matching-engine \
        -p matching-solver \
        -p matching-scenarios \
        -p matching-sim \
        -p matching-sequencer \
        -p sybil-api \
        -p sybil-api-types \
        -p sybil-oracle \
        -p sybil-verifier \
        -p sybil-zk \
        -p sybil-witgen \
        -p sybil-witgen-cli \
        -p sybil-prover \
        -p sybil-canonical \
        -p sybil-polymarket

# Build service binaries from real workspace sources.
RUN cargo build --release -p sybil-api -p sybil-polymarket -p sybil-prover

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sybil-api /usr/local/bin/sybil-api
COPY --from=builder /app/target/release/sybil-polymarket /usr/local/bin/sybil-polymarket
COPY --from=builder /app/target/release/sybil-prover /usr/local/bin/sybil-prover

EXPOSE 3000 3002

ENTRYPOINT ["sybil-api"]
