# syntax=docker/dockerfile:1

# cargo-chef supplies a manifest-driven dependency build, so workspace targets do
# not need handwritten dummy source files.
FROM rust:1.94-bookworm AS chef

RUN cargo install cargo-chef --locked

WORKDIR /app


FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder

# Default to server-safe Rust build settings for the prod Linode
# (1 vCPU / 2 GB RAM / 495 MB swap). Local compose builds can override these
# through Docker build args without making remote builds more memory-hungry.
ARG CARGO_BUILD_JOBS=1
ARG CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1
ARG TARGETARCH
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
ENV CARGO_PROFILE_RELEASE_CODEGEN_UNITS=${CARGO_PROFILE_RELEASE_CODEGEN_UNITS}

RUN apt-get update && apt-get install -y --no-install-recommends cmake libclang-dev && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json

# Cook the same package/feature combinations built below. The recipe contains
# every manifest-declared target, while the cache mounts survive layer rebuilds.
RUN --mount=type=cache,id=sybil-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=sybil-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=sybil-target-${TARGETARCH},target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json \
        -p sybil-api -p sybil-polymarket -p sybil-prover && \
    cargo chef cook --release --recipe-path recipe.json \
        -p sybil-prover --features mock-live --bin sybil-prover-mock

# Copy the real workspace only after dependencies have been cooked.
COPY . .

# /app/target is a cache mount and is not part of the image filesystem, so copy
# the completed executables to a normal directory before the mount is detached.
RUN --mount=type=cache,id=sybil-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=sybil-cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=sybil-target-${TARGETARCH},target=/app/target,sharing=locked \
    cargo build --release -p sybil-api -p sybil-polymarket -p sybil-prover && \
    cargo build --release -p sybil-prover --features mock-live --bin sybil-prover-mock && \
    install -d /app/bin && \
    install -m 0755 \
        /app/target/release/sybil-api \
        /app/target/release/sybil-polymarket \
        /app/target/release/sybil-prover \
        /app/target/release/sybil-prover-mock \
        /app/bin/


# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/bin/sybil-api /usr/local/bin/sybil-api
COPY --from=builder /app/bin/sybil-polymarket /usr/local/bin/sybil-polymarket
COPY --from=builder /app/bin/sybil-prover /usr/local/bin/sybil-prover
COPY --from=builder /app/bin/sybil-prover-mock /usr/local/bin/sybil-prover-mock

EXPOSE 3000 3002

ENTRYPOINT ["sybil-api"]
