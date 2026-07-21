# `deploy`

Read `DEPLOY.md` and [[Deployment Profiles]] before changing deployment state.

- The live prelaunch target is `ssh patty` (`friend@62.171.170.238`) with Sybil
  under `/opt/sybil`. `172.104.31.54` is the former rollback host, not the
  default monitoring or deployment target.
- The host is shared. Do not modify `unbiased.service`,
  `perestroika-api.service`, PostgreSQL state, unrelated nginx sites, or host
  packages; do not reboot without explicit owner approval.
- Compose files and `justfile` recipes are executable truth; incident reports
  and design documents are not deployment instructions.
- `validity-pins.json` distinguishes desired commitments from verified deployed
  evidence. `pending_redeploy` never means the adapters were repinned.
- Update validity records through the `validity-*` recipes. A validity-input
  change also requires an explicit migration or fresh-genesis decision.
- Keep secrets in host-side env files. State-reset recipes are destructive and
  require explicit operator intent.
