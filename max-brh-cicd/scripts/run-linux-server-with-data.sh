#!/bin/bash
# Runs a Linux server binary from the template with the full original game data folder, so the
# whole startup path is exercised: resources, palette, lobby, enet bind.
#
# usage: run-linux-server-with-data.sh <template-dir> [binary-override]
#
# Invoked as: docker run --rm \
#     -v "D:/dev/max-port-debug:/src:ro" \
#     -v "D:/dev/max-port-debug/bin/linux-server-template:/srv:ro" \
#     ubuntu:24.04 bash /src/build/run-linux-server-with-data.sh /srv
set -u

SRC="${1:-/srv}"
BINARY="${2:-}"
GAMEDATA="${MAX_GAME_DATA:-/src/max-original}"
BASE=$(mktemp -d /tmp/max-server.XXXXXX)

cp -a "$SRC"/. "$BASE"/

if [ -n "$BINARY" ]; then
    cp -f "$BINARY" "$BASE/max-port"
    chmod +x "$BASE/max-port"
fi

# Point the game at the original game data folder; the shipped settings.json points at a
# Windows path that does not exist in a container. Horizon does the same for a session.
sed -i "s|\"game_data\": *\"[^\"]*\"|\"game_data\": \"$GAMEDATA\"|" "$BASE/settings.json"

cd "$BASE" || exit 1

echo "=== run dir: $BASE ==="
grep -m1 'game_data' settings.json
sha256sum max-port

echo "=== server run (25s) ==="
# The server catches SIGTERM, so the timeout needs a SIGKILL follow-up.
timeout -k 5 25 env LD_LIBRARY_PATH=. ./max-port --server > server.out 2>&1
echo "exit=$? (137 = SIGKILL after the run window, expected)"

echo "--- first 50 lines of output ---"
sed -n '1,50p' server.out

echo "=== markers ==="
grep -E 'SERVER: (entry|initializing resources|resources ready)' server.out || echo "NO SERVER MARKERS"
