# sybil-custody

User-side custody tooling for retaining an escape floor and proving a claim
without the operator's prover. The installed binary is `sybil-custody`.

```bash
# Periodic two-file self-insurance. Supplying L1 arguments stores the matching
# accepted RootRecord beside the small DA manifest.
sybil-custody snapshot \
  --api-url http://127.0.0.1:3000 --account-id 7 \
  --rpc-url http://127.0.0.1:8545 --settlement 0x...

# Full SYB-80 section 3 verification. The payload may be a saved local file or
# fetched live; an own-leaf snapshot alone is deliberately not called a full
# reconstruction.
sybil-custody reconstruct --height 42 --account-id 7 \
  --snapshot custody-proof.json --manifest custody-manifest.json \
  --api-url http://127.0.0.1:3000 \
  --rpc-url http://127.0.0.1:8545 --settlement 0x...

# Form-L escape. By default this writes OpenVM input, runs EVM prove + verify,
# ABI-wraps the proof, and prints escapeClaim calldata. Add --submit plus an
# Ethereum private key to broadcast it.
sybil-custody escape-claim --snapshot custody-proof.json \
  --rpc-url http://127.0.0.1:8545 --settlement 0x... --vault 0x... \
  --recipient 0x... --p256-private-key "$SYBIL_P256_PRIVATE_KEY"
```

`SYBIL_API_TOKEN` supplies the bearer required by the current state-proof and
DA-payload API surfaces. The P256 scalar authorizes the guest statement; it is
not an Ethereum transaction key. Real EVM proving is intentionally never part
of default tests. The hidden fixture-proof flag is only for the throwaway Anvil
accept-all adapter used by `scripts/itest-compose.sh`.
