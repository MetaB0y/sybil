#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
report="$(mktemp)"
trap 'rm -f "$report"' EXIT

(
    cd "$repo_root/contracts"
    forge coverage \
        --report lcov \
        --report-file "$report" \
        --exclude-tests \
        --no-match-coverage '(^|/)(script|src/dev)/'
)

awk '
BEGIN {
    count = 4
    files[1] = "src/OpenVmVerifierAdapter.sol"
    files[2] = "src/SybilSettlement.sol"
    files[3] = "src/SybilVault.sol"
    files[4] = "src/access/SybilAccessControl.sol"

    minimum[files[1]] = 85
    minimum[files[2]] = 90
    minimum[files[3]] = 90
    minimum[files[4]] = 75
    aggregate_minimum = 95
}

/^SF:/ {
    current = substr($0, 4)
    seen[current] = 1
    next
}

/^BRF:/ {
    found[current] = substr($0, 5) + 0
    next
}

/^BRH:/ {
    hit[current] = substr($0, 5) + 0
    next
}

END {
    failed = 0
    aggregate_hit = 0
    aggregate_found = 0

    for (i = 1; i <= count; i++) {
        file = files[i]
        if (!seen[file] || found[file] == 0) {
            printf "contract coverage: missing branch data for %s\n", file > "/dev/stderr"
            failed = 1
            continue
        }

        basis_points = int((hit[file] * 10000 + found[file] / 2) / found[file])
        printf "contract coverage: %-37s %d/%d branches (%d.%02d%%, floor %d%%)\n",
            file,
            hit[file],
            found[file],
            int(basis_points / 100),
            basis_points % 100,
            minimum[file]

        if (hit[file] * 100 < minimum[file] * found[file]) {
            printf "contract coverage: %s is below its %d%% branch floor\n",
                file, minimum[file] > "/dev/stderr"
            failed = 1
        }
        aggregate_hit += hit[file]
        aggregate_found += found[file]
    }

    if (aggregate_found == 0) {
        printf "contract coverage: no aggregate branch data\n" > "/dev/stderr"
        exit 1
    }

    aggregate_basis_points = int((aggregate_hit * 10000 + aggregate_found / 2) / aggregate_found)
    printf "contract coverage: %-37s %d/%d branches (%d.%02d%%, floor %d%%)\n",
        "aggregate production boundary",
        aggregate_hit,
        aggregate_found,
        int(aggregate_basis_points / 100),
        aggregate_basis_points % 100,
        aggregate_minimum

    if (aggregate_hit * 100 < aggregate_minimum * aggregate_found) {
        printf "contract coverage: aggregate production boundary is below its %d%% branch floor\n",
            aggregate_minimum > "/dev/stderr"
        failed = 1
    }
    exit failed
}
' "$report"
