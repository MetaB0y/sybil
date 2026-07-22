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
| Witness format | `13` |
| Empty canonical witness length | `1631` bytes |
| Golden-vector schema | `7` |
| Canonical witness vector length | `4034` bytes |
| Canonical witness length-prefixed SHA-256 | `0x83707666be8f785033b2c9e60f06e18382d34b31edce1d7f109fb96c47185628` |
| Transition public-input hash | `0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` |
| Empty transition public-input hash | `0x487afe369cfd9c388069042c1736df9752a84e30654da2e445b23f2d10ea146c` |
| Epoch transition public-input hash | `0x7ef3a4d5373c7da2b5a6952006aff3b0d7b84b3eedfae7fa3095da5b7f3a532d` |
| Escape public-input hash | `0x35e754909d75fe0658f0bc451471d85f048110d7950b139110e7cd2e90ffb5d5` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `0x003b701ac6da570efa91be160499dd26c7b6cd88182fdf17b16e043f3a98b973` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |
| Escape claim | `0x00690871523a5e7c63c2bbe796aa312347b34a1427293b7b7e1c0f5c152f013f` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `devnet` | `pending_redeploy` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
