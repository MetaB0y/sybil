FROM python:3.12-slim

ARG SYBIL_SOURCE_REVISION=unknown
LABEL org.opencontainers.image.revision="${SYBIL_SOURCE_REVISION}"

RUN apt-get update && apt-get install -y --no-install-recommends curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Keep the resolver/build tool reproducible across image rebuilds.
COPY --from=ghcr.io/astral-sh/uv:0.11.28 /uv /usr/local/bin/uv

# Copy project files
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev

# Copy source
COPY sybil_client/ sybil_client/
COPY bots/ bots/
COPY sim/ sim/
COPY live/ live/
COPY scripts/ scripts/
COPY feeds/ feeds/
COPY markets/ markets/

EXPOSE 8501 9101 9103

# Default: run the live trading bots
ENTRYPOINT ["uv", "run", "--no-sync", "python", "-m", "live.runner"]
