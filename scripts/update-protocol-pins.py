#!/usr/bin/env python3
"""Generate or verify the compact protocol-pins documentation page."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "docs/protocol-pins.md"


def load_json(relative: str) -> dict:
    return json.loads((ROOT / relative).read_text(encoding="utf-8"))


def witness_version() -> int:
    source = (ROOT / "crates/sybil-verifier/src/witness_schema.rs").read_text(
        encoding="utf-8"
    )
    match = re.search(r"WITNESS_FORMAT_VERSION:\s*u8\s*=\s*(\d+)", source)
    if match is None:
        raise RuntimeError("WITNESS_FORMAT_VERSION not found")
    return int(match.group(1))


def empty_witness_length() -> int:
    source = (ROOT / "crates/sybil-verifier/src/witness_schema.rs").read_text(
        encoding="utf-8"
    )
    match = re.search(
        r"fn canonical_witness_bytes_are_stable_for_empty_witness\(\).*?"
        r"assert_eq!\(bytes\.len\(\),\s*(\d+)\)",
        source,
        re.DOTALL,
    )
    if match is None:
        raise RuntimeError("empty canonical witness length pin not found")
    return int(match.group(1))


def empty_public_input_hash() -> str:
    source = (ROOT / "crates/sybil-zk/src/lib.rs").read_text(encoding="utf-8")
    match = re.search(
        r"fn public_input_hash_golden\(\).*?assert_eq!\(.*?,\s*\[(.*?)\]\s*\)",
        source,
        re.DOTALL,
    )
    if match is None:
        raise RuntimeError("empty public-input hash pin not found")
    values = [int(value) for value in re.findall(r"\d+", match.group(1))]
    if len(values) != 32 or any(value > 255 for value in values):
        raise RuntimeError("empty public-input hash pin is not 32 bytes")
    return "0x" + bytes(values).hex()


def render() -> str:
    golden = load_json("golden/golden-vectors.json")
    main = load_json("zk/openvm-guest/openvm/release/sybil-openvm-guest.commit.json")
    escape = load_json(
        "zk/openvm-escape-guest/openvm/release/sybil-openvm-escape-guest.commit.json"
    )
    deployment = load_json("deploy/validity-pins.json")

    return f"""---
tags: [reference, generated, verification]
status: current
---

# Current protocol pins

> **Generated file:** run `just docs-pins-write` after an intentional protocol
> change. `just docs-check` fails when this page differs from source artifacts.

## Formats and shared vectors

| Pin | Value |
|---|---:|
| Witness format | `{witness_version()}` |
| Empty canonical witness length | `{empty_witness_length()}` bytes |
| Golden-vector schema | `{golden["schema_version"]}` |
| Canonical witness vector length | `{golden["canonical_witness"]["length"]}` bytes |
| Canonical witness length-prefixed SHA-256 | `{golden["canonical_witness"]["length_prefixed_sha256"]}` |
| Transition public-input hash | `{golden["state_transition_public_inputs"]["hash"]}` |
| Empty transition public-input hash | `{empty_public_input_hash()}` |
| Escape public-input hash | `{golden["escape_claim_public_inputs"]["hash"]}` |

## OpenVM guest commitments

| Guest | Executable commitment | VM commitment |
|---|---|---|
| State transition | `{main["app_exe_commit"]}` | `{main["app_vm_commit"]}` |
| Escape claim | `{escape["app_exe_commit"]}` | `{escape["app_vm_commit"]}` |

## Deployment coordination

| Network | Recorded status |
|---|---|
| `{deployment["network"]}` | `{deployment["status"]}` |

Sources: `witness_schema.rs`, `golden/golden-vectors.json`, and the two committed
OpenVM release `commit.json` files. `deploy/validity-pins.json` separately binds
those desired pins to explicit deployment evidence; a `pending_redeploy` status
must never be interpreted as an on-chain repin.
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()

    expected = render()
    if args.write:
        OUTPUT.write_text(expected, encoding="utf-8")
        print(f"wrote {OUTPUT.relative_to(ROOT)}")
        return 0

    actual = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else ""
    if actual != expected:
        print(
            "docs/protocol-pins.md is stale; run `just docs-pins-write`",
            file=sys.stderr,
        )
        return 1
    print("protocol pins match source artifacts")
    return 0


if __name__ == "__main__":
    sys.exit(main())
