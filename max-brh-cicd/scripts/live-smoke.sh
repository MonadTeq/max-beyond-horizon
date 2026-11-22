#!/usr/bin/env bash
# Repeatable live smoke: Horizon (WS+gRPC) + C++ headless + 2 Rust clients × N turns.
# Run from outside (Git Bash/WSL/Linux) or from inside a container with docker:
#   bash max-brh-cicd/scripts/live-smoke.sh [turns]
# Requirements: the workspace is mounted as /src (HOST_ROOT -> /src);
# built: max-brh-server/target/release/horizon, bin/linux-server-template/max-port.
set -euo pipefail
TURNS="${1:-5}"
WS="/src"
HOST_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
# Docker Desktop for Windows expects D:/..., not /d/...
if command -v cygpath >/dev/null 2>&1; then
  HOST_ROOT="$(cygpath -m "$HOST_ROOT")"
fi
LOGS="$HOST_ROOT/build/smoke-logs"

cleanup() { docker rm -f horizon-live client-a client-b 2>/dev/null || true; }
trap cleanup EXIT

cleanup
mkdir -p "$LOGS"

# 1. Horizon
docker run -d --name horizon-live -v "$HOST_ROOT:/src" -w "$WS/max-brh-server" \
  -p 8082:8082 -p 8080:8080 ubuntu:24.04 bash -c \
  "apt-get update -qq && apt-get install -y -qq libssl3t64 libgcc-s1 libstdc++6 >/dev/null && ./target/release/horizon --config smoke_config.toml"
sleep 8

# 2. Create a game (gRPC probe), grab the id
docker run --rm --network=container:horizon-live -v "$HOST_ROOT:/src" -w "$WS" \
  rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo run -q --no-default-features --manifest-path $WS/max-brh/Cargo.toml -p max_app --example horizon_probe -- 127.0.0.1 8082 smoke 2>/dev/null | grep create_game | sed 's/.*id=//;s/ name=.*//'" \
  > "$LOGS/game_id.txt" || true
GAME_ID="$(cat "$LOGS/game_id.txt")"
if [ -z "$GAME_ID" ]; then
  echo "live-smoke: horizon_probe не вернул create_game (см. логи выше)" >&2
  exit 1
fi
echo "GAME_ID=$GAME_ID"

# 3. Two Rust clients (shared network namespace)
#    --handshake-patches is NOT passed: the client tries the full data_hash by itself
#    (MAX.RES+PATCHES.RES), gets TP03 GameData and reconnects with the hash
#    of MAX.RES only — the way the server computes it without PATCHES.RES in game_data.
docker run -d --rm --name client-a --network=container:horizon-live -v "$HOST_ROOT:/src" -w "$WS" \
  rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && timeout 900 cargo run -q --no-default-features --manifest-path $WS/max-brh/Cargo.toml -p max_app -- --connect 127.0.0.1:8082 --join $GAME_ID --password '' --player rustA --team 0 --data-dir $WS/max-original --assets-dir $WS/assets --turns $TURNS > $WS/build/smoke-logs/a.log 2>&1"
docker run -d --rm --name client-b --network=container:horizon-live -v "$HOST_ROOT:/src" -w "$WS" \
  rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && sleep 8 && timeout 900 cargo run -q --no-default-features --manifest-path $WS/max-brh/Cargo.toml -p max_app -- --connect 127.0.0.1:8082 --join $GAME_ID --password '' --player rustB --team 1 --data-dir $WS/max-original --assets-dir $WS/assets --turns $TURNS > $WS/build/smoke-logs/b.log 2>&1"

# 4. Wait for the clients to finish (max ~16 min)
for i in $(seq 1 96); do
  if ! docker inspect client-a >/dev/null 2>&1 && ! docker inspect client-b >/dev/null 2>&1; then break; fi
  sleep 10
done

echo "== client A =="; tail -6 "$LOGS/a.log"
echo "== client B =="; tail -6 "$LOGS/b.log"

# 5. fallback data_hash check: a server without PATCHES.RES accepts only
#    the 64-character hash (MAX.RES), the client reconnects on its own, without a flag.
fallback_ok=0
for log in "$LOGS/a.log" "$LOGS/b.log"; do
  if grep -q 'data_hash=[0-9a-f]*(64)' "$log"; then
    fallback_ok=1
  fi
  if grep -q 'Session failed' "$log"; then
    echo "live-smoke: $log: сессия упала (см. лог)" >&2
    exit 1
  fi
done
if [ "$fallback_ok" -ne 1 ]; then
  echo "live-smoke: fallback data_hash (64 hex) не сработал — проверьте TP03 GameData в логах" >&2
  exit 1
fi
echo "live-smoke: fallback data_hash (MAX.RES-only) подтверждён"
