#!/usr/bin/env bash
set -euo pipefail

# Host-side production smoke check.
# Run on the deploy host from /opt/sybil after compose services are up.

allowed_ports="${OPS_SMOKE_ALLOWED_PUBLIC_PORTS:-80 443}"
secret_regex="${OPS_SMOKE_SECRET_REGEX:-sk-or-[A-Za-z0-9_-]+}"

tmpdir="$(mktemp -d)"
cleanup() {
    rm -rf "$tmpdir"
}
trap cleanup EXIT

is_allowed_port() {
    local port=$1
    local allowed
    for allowed in $allowed_ports; do
        [[ "$port" == "$allowed" ]] && return 0
    done
    return 1
}

is_loopback_v4_hex() {
    local addr=$1
    # /proc/net/tcp stores IPv4 addresses little-endian; 127/8 ends in 7F.
    [[ "$addr" == *7F ]]
}

is_loopback_v6_hex() {
    local addr=$1
    # Linux /proc/net/tcp6 loopback representation for ::1.
    [[ "$addr" == "00000000000000000000000001000000" ]]
}

scan_tcp_table() {
    local file=$1
    local family=$2

    [[ -r "$file" ]] || return 0

    awk 'NR > 1 && $4 == "0A" { print $2 }' "$file" | while IFS=: read -r addr port_hex; do
        [[ -n "$addr" && -n "$port_hex" ]] || continue

        if [[ "$family" == "tcp4" ]] && is_loopback_v4_hex "$addr"; then
            continue
        fi
        if [[ "$family" == "tcp6" ]] && is_loopback_v6_hex "$addr"; then
            continue
        fi

        port=$((16#$port_hex))
        if ! is_allowed_port "$port"; then
            printf "%s %s:%s\n" "$family" "$addr" "$port"
        fi
    done
}

public_findings="$tmpdir/public_ports.txt"
{
    scan_tcp_table /proc/net/tcp tcp4
    scan_tcp_table /proc/net/tcp6 tcp6
} | sort -u > "$public_findings"

status=0

if [[ -s "$public_findings" ]]; then
    echo "Unexpected public listening ports found (allowed: $allowed_ports):" >&2
    sed 's/^/  /' "$public_findings" >&2
    status=1
else
    echo "Public listening ports OK (allowed: $allowed_ports)"
fi

ps_out="$tmpdir/ps.txt"
ps -eo pid=,args= > "$ps_out"
if grep -E "$secret_regex" "$ps_out" >/dev/null; then
    echo "Secret-like material found in process arguments:" >&2
    grep -En "$secret_regex" "$ps_out" \
        | sed -E "s/$secret_regex/sk-or-REDACTED/g; s/^/  /" >&2
    status=1
else
    echo "Process arguments OK"
fi

inspect_out="$tmpdir/docker-command-arrays.txt"
if command -v docker >/dev/null 2>&1; then
    container_ids_file="$tmpdir/container-ids.txt"
    if ! docker ps -q > "$container_ids_file"; then
        echo "docker ps failed" >&2
        status=1
    else
        if [[ -s "$container_ids_file" ]]; then
            # Check command-bearing fields only, not container environment values.
            docker inspect \
                --format '{{.Name}} entrypoint={{json .Config.Entrypoint}} cmd={{json .Config.Cmd}} path={{json .Path}} args={{json .Args}}' \
                $(cat "$container_ids_file") > "$inspect_out"
        else
            : > "$inspect_out"
        fi

        if grep -E "$secret_regex" "$inspect_out" >/dev/null; then
            echo "Secret-like material found in docker inspect command arrays:" >&2
            grep -En "$secret_regex" "$inspect_out" \
                | sed -E "s/$secret_regex/sk-or-REDACTED/g; s/^/  /" >&2
            status=1
        else
            echo "Docker command arrays OK"
        fi
    fi
else
    echo "docker CLI not found" >&2
    status=1
fi

exit "$status"
