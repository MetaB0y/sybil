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
| Witness format | `11` |
| Empty canonical witness length | `1631` bytes |
| Golden-vector schema | `5` |
| Canonical witness vector length | `3984` bytes |
| Canonical witness length-prefixed SHA-256 | `0x4609379c38ec46b129c7bc3e3bf8f47f5095d07b9afe11df604b0c5022faf2a3` |
| Transition public-input hash | `0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` |
| Empty transition public-input hash | `0x8d50c79301cd188e9c7ce00933af2acb4cecdf3807d86b1955bbecdb55a6a8fa` |
| Epoch transition public-input hash | `0x7ef3a4d5373c7da2b5a6952006aff3b0d7b84b3eedfae7fa3095da5b7f3a532d` |
| Escape public-input hash | `0x35e754909d75fe0658f0bc451471d85f048110d7950b139110e7cd2e90ffb5d5` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `0x0079b1a181ea4ac954db8c19c1cd74382b8a6cb1c4c86348f74152b0777df961` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |
| Escape claim | `0x0097bc9bfbbeafe1005708bb5ec1cff2c5a310532f542304d309d1ab6479a6cf` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `devnet` | `pending_redeploy` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
