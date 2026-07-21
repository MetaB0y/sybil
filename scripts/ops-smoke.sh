#!/usr/bin/env bash
set -euo pipefail

# Shared-host-safe production smoke check.
# Run on the deploy host from /opt/sybil after compose services are up. Inspect
# only the Sybil Compose project; unrelated host services are outside its
# ownership boundary.

compose_project="${OPS_SMOKE_COMPOSE_PROJECT:-sybil}"
secret_regex="${OPS_SMOKE_SECRET_REGEX:-sk-or-[A-Za-z0-9_-]+}"

tmpdir="$(mktemp -d)"
cleanup() {
    rm -rf "$tmpdir"
}
trap cleanup EXIT

status=0

inspect_out="$tmpdir/docker-command-arrays.txt"
if command -v docker >/dev/null 2>&1; then
    container_ids_file="$tmpdir/container-ids.txt"
    if ! docker ps -q \
        --filter "label=com.docker.compose.project=$compose_project" \
        > "$container_ids_file"; then
        echo "docker ps failed for Compose project $compose_project" >&2
        status=1
    elif [[ ! -s "$container_ids_file" ]]; then
        echo "No running containers found for Compose project $compose_project" >&2
        status=1
    else
        mapfile -t container_ids < "$container_ids_file"
        network_modes="$tmpdir/docker-network-modes.txt"
        port_bindings="$tmpdir/docker-port-bindings.txt"
        public_findings="$tmpdir/public-bindings.txt"

        if ! docker inspect \
            --format '{{println .Name .HostConfig.NetworkMode}}' \
            "${container_ids[@]}" > "$network_modes"; then
            echo "docker inspect failed while reading Sybil network modes" >&2
            status=1
        else
            awk '$2 == "host" { print $1 " network_mode=host" }' \
                "$network_modes" > "$public_findings"
        fi

        if ! docker inspect \
            --format '{{range $port, $bindings := .NetworkSettings.Ports}}{{range $bindings}}{{println $.Name $port .HostIp .HostPort}}{{end}}{{end}}' \
            "${container_ids[@]}" \
            | sed '/^[[:space:]]*$/d' > "$port_bindings"; then
            echo "docker inspect failed while reading Sybil port bindings" >&2
            status=1
        else
            while read -r name container_port host_ip host_port; do
                [[ -n "$name" ]] || continue
                case "$host_ip" in
                    127.*|"::1"|"[::1]") ;;
                    *)
                        printf '%s %s -> %s:%s\n' \
                            "$name" "$container_port" "$host_ip" "$host_port" \
                            >> "$public_findings"
                        ;;
                esac
            done < "$port_bindings"
        fi

        if [[ -s "$public_findings" ]]; then
            echo "Unexpected public Sybil Docker exposure (project: $compose_project):" >&2
            sed 's/^/  /' "$public_findings" >&2
            status=1
        else
            echo "Sybil Docker bindings are loopback-only (project: $compose_project)"
        fi

        # Check command-bearing fields only, never container environment values.
        if ! docker inspect \
            --format '{{.Name}} entrypoint={{json .Config.Entrypoint}} cmd={{json .Config.Cmd}} path={{json .Path}} args={{json .Args}}' \
            "${container_ids[@]}" > "$inspect_out"; then
            echo "docker inspect failed while reading Sybil command arrays" >&2
            status=1
        elif grep -E "$secret_regex" "$inspect_out" >/dev/null; then
            echo "Secret-like material found in Sybil Docker command arrays:" >&2
            grep -En "$secret_regex" "$inspect_out" \
                | sed -E "s/$secret_regex/sk-or-REDACTED/g; s/^/  /" >&2
            status=1
        else
            echo "Sybil Docker command arrays OK"
        fi
    fi
else
    echo "docker CLI not found" >&2
    status=1
fi

exit "$status"
