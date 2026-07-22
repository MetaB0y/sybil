---
tags: [reference, generated, verification]
status: current
---

# Current protocol pins

> **Generated file:** run `just docs-pins-write` after an intentional protocol
> change. `just docs-check` fails when this page differs from source artifacts.

## Formats and shared vectors

| Pin | Value |
|---|---:|
| Witness format | `14` |
| Empty canonical witness length | `1631` bytes |
| Golden-vector schema | `7` |
| Canonical witness vector length | `4034` bytes |
| Canonical witness length-prefixed SHA-256 | `0x66888c6f17ee568dcdfd43b68f300e93cec16c0e6d7288e3acf55f3ca47f5335` |
| Transition public-input hash | `0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` |
| Empty transition public-input hash | `0xd7fb575f359fc1ed251c378f1d1f988b27cb90d1eef19fcb07dd6aa7a61beb8e` |
| Epoch transition public-input hash | `0x7ef3a4d5373c7da2b5a6952006aff3b0d7b84b3eedfae7fa3095da5b7f3a532d` |
| Escape public-input hash | `0x35e754909d75fe0658f0bc451471d85f048110d7950b139110e7cd2e90ffb5d5` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `0x0060f5d194b975e3e513396bb23c8fb8b047a9f78e5b61884eb2b31e1fab6f4e` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |
| Escape claim | `0x0042bd996a1b16e62053164e0f5080e8be33aadba10ebac862def073d2fdee6f` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `devnet` | `pending_redeploy` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
