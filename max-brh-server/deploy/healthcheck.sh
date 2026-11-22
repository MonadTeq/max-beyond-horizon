#!/bin/bash
# M5.3: healthcheck without opening TCP connections (a raw connect would pollute the
# Horizon log with a WS handshake error). We look at LISTEN sockets in /proc/net/tcp*.
set -eu

check_port() {
    local port="$1"
    local hex
    hex=$(printf '%04X' "$port")
    awk -v hex="$hex" '
        FNR == 1 { next }
        {
            split($2, addr, ":")
            if (toupper(addr[2]) == hex && $4 == "0A") { found = 1 }
        }
        END { exit(found ? 0 : 1) }
    ' /proc/net/tcp /proc/net/tcp6
}

for port in 8080 8082; do
    if ! check_port "$port"; then
        echo "healthcheck: порт $port не слушается" >&2
        exit 1
    fi
done

echo "healthcheck: WS 8080 и gRPC 8082 слушаются"
