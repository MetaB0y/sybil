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
| Golden-vector schema | `5` |
| Canonical witness vector length | `4246` bytes |
| Canonical witness length-prefixed SHA-256 | `0xca392e3f59a2ac1733a8dad184b4beb91c663672f0a9eb9ad6399a063928dd92` |
| Transition public-input hash | `0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` |
| Empty transition public-input hash | `0xc0ca0cb7dd000a6362eafd24307ef0edbf6aa6399973784b71062ba279a17509` |
| Epoch transition public-input hash | `0x7ef3a4d5373c7da2b5a6952006aff3b0d7b84b3eedfae7fa3095da5b7f3a532d` |
| Escape public-input hash | `0x35e754909d75fe0658f0bc451471d85f048110d7950b139110e7cd2e90ffb5d5` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `0x0029c549755b05fc91ce327c57b0eb995933271d8477348cba35724cb2a6b305` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |
| Escape claim | `0x0050a798448ab3a198a2a04515a01a37a726f160d4add775bd6a87297afc6b97` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `devnet` | `pending_redeploy` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
