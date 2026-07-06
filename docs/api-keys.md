# API Keys

**Do not commit live secrets to this repo.** Keys live in the deploy host's
`/opt/sybil/.env` (see `docs/runbooks/`) and each operator's local environment,
never in tracked files. This page documents *which* keys exist and *where* they
are sourced — not their values.

## OpenRouter

Arena bot LLM calls (`deepseek/deepseek-v4-flash` model).

- Source: `OPENROUTER_API_KEY` in the deploy host env / local shell.
- Used by: `just deploy-arena` (reads the key from the environment).
- Rotation / custody: see the Valery action list (SYB-218) and SYB-230.

> A previously committed key was removed from this file (SYB-230). Because it
> remains in git history, it must be **rotated**, not just deleted — treat the
> old value as compromised.
