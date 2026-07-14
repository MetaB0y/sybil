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
| Witness format | `10` |
| Empty canonical witness length | `1631` bytes |
| Golden-vector schema | `4` |
| Canonical witness vector length | `4246` bytes |
| Canonical witness length-prefixed SHA-256 | `0xca392e3f59a2ac1733a8dad184b4beb91c663672f0a9eb9ad6399a063928dd92` |
| Transition public-input hash | `0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` |
| Empty transition public-input hash | `0xc0ca0cb7dd000a6362eafd24307ef0edbf6aa6399973784b71062ba279a17509` |
| Escape public-input hash | `0x35e754909d75fe0658f0bc451471d85f048110d7950b139110e7cd2e90ffb5d5` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `0x0018b4390609b3e57883160fbec353413cda5982cef98b32d340820e8bdeb30d` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |
| Escape claim | `0x006af4f629b4e749ee6b2a0f717dadbf651f6639b305b1482b8ea5f98ef539c5` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `devnet` | `pending_redeploy` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
