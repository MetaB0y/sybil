FROM python:3.12-slim

RUN apt-get update && apt-get install -y --no-install-recommends curl && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Install uv for fast dependency resolution
COPY --from=ghcr.io/astral-sh/uv:latest /uv /usr/local/bin/uv

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

EXPOSE 8501

# Default: run the live trading bots
ENTRYPOINT ["uv", "run", "--no-sync", "python", "-m", "live.runner"]
