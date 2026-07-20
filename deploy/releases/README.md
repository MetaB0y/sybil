# Deployment records

Each successful application promotion writes a `sybil.release.v1` JSON record
here. Commit that record after the deployment so the deployed image references,
image IDs, source revisions, and verification time exist outside the host.

The host keeps the matching `images.env` under `/opt/sybil/releases/<release>/`
and atomically points `/opt/sybil/releases/current.env` at the active set.
Rollback selects one of those recorded sets and never rebuilds an image.

These records contain artifact identity only. Never put credentials or host
environment values in this directory.
