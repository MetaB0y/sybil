# syntax=docker/dockerfile:1
#
# Sybil web frontend (Next.js) — multi-stage, standalone output, non-root.
# Build context is this directory (frontend/web).
#
#   docker build \
#     --build-arg NEXT_PUBLIC_API_BASE=https://172-104-31-54.nip.io \
#     --build-arg NEXT_PUBLIC_WS_BASE=wss://172-104-31-54.nip.io \
#     -t sybil-web:latest frontend/web
#
# IMPORTANT: NEXT_PUBLIC_* are inlined into the client bundle at BUILD time.
# They are build args, NOT runtime env — changing them requires a rebuild.

ARG NODE_VERSION=22

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

# Public (build-time-baked) config. Defaults target the live nip.io backend;
# override per environment. WEBAUTHN_RP_ID must match the public domain that
# serves this app (passkeys are scoped to the rpId) — leave empty to use the
# page's own host, or set it explicitly when the app is on a subdomain.
ARG NEXT_PUBLIC_API_BASE=https://172-104-31-54.nip.io
ARG NEXT_PUBLIC_WS_BASE=wss://172-104-31-54.nip.io
ARG NEXT_PUBLIC_WEBAUTHN_RP_ID=
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
