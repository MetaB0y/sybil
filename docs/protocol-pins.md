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
| Witness format | `12` |
| Empty canonical witness length | `1631` bytes |
| Golden-vector schema | `7` |
| Canonical witness vector length | `4034` bytes |
| Canonical witness length-prefixed SHA-256 | `0x088607e7788c57c3760073e3aaa959a778c1bc42da7d6a970c830cbe03fb98e0` |
| Transition public-input hash | `0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` |
| Empty transition public-input hash | `0x487afe369cfd9c388069042c1736df9752a84e30654da2e445b23f2d10ea146c` |
| Epoch transition public-input hash | `0x7ef3a4d5373c7da2b5a6952006aff3b0d7b84b3eedfae7fa3095da5b7f3a532d` |
| Escape public-input hash | `0x35e754909d75fe0658f0bc451471d85f048110d7950b139110e7cd2e90ffb5d5` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `0x00837ae158d26c9265be2096f50d2864efb51225c116ddcee794dc8294d45369` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |
| Escape claim | `0x00026aa313d5f7c0158118be7a9ed89883625bb6b9622f0976377c0ae9173351` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `devnet` | `pending_redeploy` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
