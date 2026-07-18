# `deploy`

Read `DEPLOY.md` and [[Deployment Profiles]] before changing deployment state.

- Compose files and `justfile` recipes are executable truth; incident reports
  and design documents are not deployment instructions.
- `validity-pins.json` distinguishes desired commitments from verified deployed
  evidence. `pending_redeploy` never means the adapters were repinned.
- Update validity records through the `validity-*` recipes. A validity-input
  change also requires an explicit migration or fresh-genesis decision.
- Keep secrets in host-side env files. State-reset recipes are destructive and
  require explicit operator intent.
