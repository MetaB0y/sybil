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
| Witness format | `9` |
| Empty canonical witness length | `1631` bytes |
| Golden-vector schema | `4` |
| Canonical witness vector length | `4003` bytes |
| Canonical witness length-prefixed SHA-256 | `0xb37ef5be6703feddaf24ac8075851e62701c236464c0cf9a72bf5d41fc299a13` |
| Transition public-input hash | `0x42197d0dff7bc2f86a6e359f187adda163fc9b4ffaa0e7cfb9845561bb744830` |
| Empty transition public-input hash | `0x3c2b17b07142b39b143af5ced9497325248be056cc96ff2daf8845b52cc76c46` |
| Escape public-input hash | `0x35e754909d75fe0658f0bc451471d85f048110d7950b139110e7cd2e90ffb5d5` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `0x004f2f9b1f6115f4befa7127ded102fd26b1a62c66e21868f2f1e50c5f4f09b5` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |
| Escape claim | `0x0013128cbb4f1579cc3cd3df99bfe0293684a703d78d6c359c1480642a5c08f0` | `0x006185384dcac8a449ebcad26ce224c07145ad440e4739b237439a4318d3cd9d` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `devnet` | `pending_redeploy` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
