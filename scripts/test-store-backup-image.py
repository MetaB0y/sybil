#!/usr/bin/env python3
"""Guard the backup validator against following a mutable configured tag."""

from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
SCRIPT = (ROOT / "scripts/store-backup.sh").read_text(encoding="utf-8")


def require(fragment: str, message: str) -> None:
    if fragment not in SCRIPT:
        raise SystemExit(f"FAIL: {message}")


require(
    "SOURCE_IMAGE=\"$(docker inspect --format '{{.Config.Image}}' \"$CONTAINER\")\"",
    "backup no longer records the configured source image reference",
)
require(
    "SOURCE_IMAGE_ID=\"$(docker inspect --format '{{.Image}}' \"$CONTAINER\")\"",
    "backup does not capture the running container's immutable image ID",
)
require(
    '--entrypoint sybil-api "$SOURCE_IMAGE_ID"',
    "isolation validation does not boot the immutable runtime image",
)
require(
    'SOURCE_RETAIN_VALIDITY_ARTIFACTS="$(',
    "backup does not capture the running chain validity-retention mode",
)
require(
    '-e SYBIL_RETAIN_VALIDITY_ARTIFACTS="$SOURCE_RETAIN_VALIDITY_ARTIFACTS"',
    "isolation validation can boot with a chain-incompatible validity mode",
)
if '--entrypoint sybil-api "$SOURCE_IMAGE"' in SCRIPT:
    raise SystemExit("FAIL: isolation validation still follows the mutable image tag")
require('--image "$SOURCE_IMAGE"', "manifest lost the configured image reference")
require('--image-id "$SOURCE_IMAGE_ID"', "manifest lost the immutable image ID")
require(
    '--retain-validity-artifacts "$SOURCE_RETAIN_VALIDITY_ARTIFACTS"',
    "manifest lost the source chain validity-retention mode",
)

print("store backup immutable-image contract: ok")
