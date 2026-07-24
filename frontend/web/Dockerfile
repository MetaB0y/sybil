# syntax=docker/dockerfile:1
#
# Sybil web frontend (Next.js) — multi-stage, standalone output, non-root.
# Build context is this directory (frontend/web).
#
#   docker build \
#     --build-arg NEXT_PUBLIC_API_BASE=https://api.sybil.exchange \
#     --build-arg NEXT_PUBLIC_WS_BASE=wss://api.sybil.exchange \
#     -t sybil-web:latest frontend/web
#
# IMPORTANT: NEXT_PUBLIC_* are inlined into the client bundle at BUILD time.
# They are build args, NOT runtime env — changing them requires a rebuild.

ARG NODE_VERSION=24

# ── deps: install node_modules from the lockfile ──────────────────────────────
FROM node:${NODE_VERSION}-slim AS deps
WORKDIR /app
ENV PNPM_HOME=/pnpm PATH=/pnpm:$PATH
RUN corepack enable
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc ./
RUN --mount=type=cache,target=/pnpm/store pnpm install --frozen-lockfile

# ── builder: compile the standalone server ────────────────────────────────────
FROM node:${NODE_VERSION}-slim AS builder
WORKDIR /app
ENV PNPM_HOME=/pnpm PATH=/pnpm:$PATH
RUN corepack enable

# Public (build-time-baked) config. Defaults target the live backend; override
# per environment.
#
# NEXT_PUBLIC_WEBAUTHN_RP_ID must equal EXPECTED_WEBAUTHN_RP_ID in
# crates/sybil-verifier/src/key_op_auth.rs, which is the registrable domain and
# NOT the `app.` host serving this page. Do not leave it empty: the browser then
# defaults the rpId to the page's own host, minting passkeys under
# `app.sybil.exchange` whose rpIdHash the guest rejects. That failure only shows
# up at the first passkey ceremony, so it is defaulted explicitly here.
ARG NEXT_PUBLIC_API_BASE=https://api.sybil.exchange
ARG NEXT_PUBLIC_WS_BASE=wss://api.sybil.exchange
ARG NEXT_PUBLIC_WEBAUTHN_RP_ID=sybil.exchange
ARG NEXT_PUBLIC_SENTRY_DSN=
ENV NEXT_PUBLIC_API_BASE=$NEXT_PUBLIC_API_BASE \
    NEXT_PUBLIC_WS_BASE=$NEXT_PUBLIC_WS_BASE \
    NEXT_PUBLIC_WEBAUTHN_RP_ID=$NEXT_PUBLIC_WEBAUTHN_RP_ID \
    NEXT_PUBLIC_SENTRY_DSN=$NEXT_PUBLIC_SENTRY_DSN \
    NEXT_TELEMETRY_DISABLED=1

COPY --from=deps /app/node_modules ./node_modules
COPY . .
# Fonts are vendored (next/font/local), so this build makes NO network calls.
RUN pnpm build

# ── runner: minimal non-root runtime ──────────────────────────────────────────
FROM node:${NODE_VERSION}-slim AS runner
WORKDIR /app
ARG SYBIL_SOURCE_REVISION=unknown
LABEL org.opencontainers.image.revision="${SYBIL_SOURCE_REVISION}"
ENV NODE_ENV=production \
    NEXT_TELEMETRY_DISABLED=1 \
    PORT=3000 \
    HOSTNAME=0.0.0.0

# node:*-slim already ships a non-root `node` user (uid 1000).
# The standalone bundle contains its own trimmed node_modules + server.js.
COPY --from=builder --chown=node:node /app/.next/standalone ./
COPY --from=builder --chown=node:node /app/.next/static ./.next/static
COPY --from=builder --chown=node:node /app/public ./public

USER node
EXPOSE 3000
CMD ["node", "server.js"]
