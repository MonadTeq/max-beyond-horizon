#!/usr/bin/env bash
# P5: смешанная сессия Rust + C++ и проверка кросс-имплементационного паритета.
#
#   bash max-brh-cicd/scripts/live-smoke-mixed.sh [turns]
#   MIXED_REQUIRE_CPP=1 bash max-brh-cicd/scripts/live-smoke-mixed.sh  # падать без C++
#
# ЧЕСТНЫЙ СТАТУС C++-КЛИЕНТА:
#   консольная сборка клиента `src/` умеет ТОЛЬКО `--server` (`src/main.cpp:37-44`,
#   `CMakeLists.txt: MAX_HEADLESS` -> `Server_Main`). Клиентской ветки
#   `--client`/`--connect` нет: обычный `max-port` — SDL GUI, которому нужен
#   дисплей и ручной проход меню. Поэтому mixed-сессия Rust+C++ этим скриптом
#   НЕ запускается; скрипт делает доступную часть паритета:
#     * два Rust-клиента в одной сессии, N ходов, CRC/state_checksum бит-в-бит;
#     * C++-сервер (эталонная реализация `state_checksum`/`GameManager_CheckDesync`)
#       на каждом 45-барьере сверяет CRC и не пишет `Desync check failed` /
#       desync-отчёт — значит, состояние Rust-клиентов совпало с C++-симуляцией.
#   Без изменения `src/` (добавления headless client mode) полноценная
#   Rust+C++-сессия невозможна; см. max-brh-docs/architecture/client/protocol.md §12 и
#   max-brh-cicd/scripts/README.md.
set -euo pipefail

TURNS="${1:-5}"
WS="/src"
HOST_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
if command -v cygpath >/dev/null 2>&1; then
  HOST_ROOT="$(cygpath -m "$HOST_ROOT")"
fi
if [ -n "${MSYSTEM:-}" ] || command -v cygpath >/dev/null 2>&1; then
  export MSYS_NO_PATHCONV=1
  export MSYS2_ARG_CONV_EXCL='*'
fi
LOGS="$HOST_ROOT/build/smoke-logs"
PREFIX="mixed"
NAMES=("${PREFIX}-a" "${PREFIX}-b")

cleanup() {
  docker rm -f "${PREFIX}-horizon" "${NAMES[@]}" >/dev/null 2>&1 || true
}
trap cleanup EXIT
cleanup
mkdir -p "$LOGS"
: > "$LOGS/${PREFIX}-a.log"
: > "$LOGS/${PREFIX}-b.log"

# Блокер фиксируем по исходнику, а не по догадке.
CPP_CLIENT_BLOCKER=""
if ! grep -q -- '--client' "$HOST_ROOT/src/main.cpp" 2>/dev/null; then
  CPP_CLIENT_BLOCKER="src/main.cpp headless: только --server (Server_Main); --client/--connect отсутствует, GUI-клиенту нужен X11 и ручной ввод"
fi
if [ -n "$CPP_CLIENT_BLOCKER" ] && [ "${MIXED_REQUIRE_CPP:-0}" = "1" ]; then
  echo "live-smoke-mixed: BLOCKED — $CPP_CLIENT_BLOCKER" >&2
  exit 1
fi

echo "== build: max_app + horizon_probe =="
docker run --rm -v "$HOST_ROOT:/src" -w "$WS/max-brh" rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && \
   cargo build -q --locked --no-default-features -p max_app --example horizon_probe"
CLIENT_BIN="$WS/max-brh/target/debug/max-brh"
PROBE_BIN="$WS/max-brh/target/debug/examples/horizon_probe"

echo "== horizon =="
docker run -d --name "${PREFIX}-horizon" -v "$HOST_ROOT:/src" -w "$WS/max-brh-server" \
  ubuntu:24.04 bash -c \
  "apt-get update -qq && apt-get install -y -qq libssl3t64 libgcc-s1 libstdc++6 >/dev/null && \
   ./target/release/horizon --config smoke_config.toml" >/dev/null

for _ in $(seq 1 30); do
  if docker exec "${PREFIX}-horizon" bash -c 'exec 3<>/dev/tcp/127.0.0.1/8082' 2>/dev/null; then
    break
  fi
  sleep 1
done
if ! docker ps --filter "name=^/${PREFIX}-horizon$" --format '{{.Names}}' | grep -q .; then
  echo "live-smoke-mixed: Horizon не запустился" >&2
  docker logs "${PREFIX}-horizon" >&2 || true
  exit 1
fi

echo "== create game (min_players=2) =="
docker run --rm --network="container:${PREFIX}-horizon" -v "$HOST_ROOT:/src" -w "$WS" \
  rust:1.94.0-bookworm bash -c \
  "$PROBE_BIN 127.0.0.1 8082 mixed 2 2>/dev/null | grep create_game | sed 's/.*id=//;s/ name=.*//'" \
  > "$LOGS/${PREFIX}-game_id.txt" || true
GAME_ID="$(tr -d '\r\n' < "$LOGS/${PREFIX}-game_id.txt")"
if [ -z "$GAME_ID" ]; then
  echo "live-smoke-mixed: horizon_probe не вернул create_game" >&2
  exit 1
fi
echo "GAME_ID=$GAME_ID"

# Rust×2 — доступная половина mixed-сессии. C++-сторона — сервер-арбитр:
# он сверяет присланные клиентами CRC на 45 и знает свою симуляцию.
for index in 0 1; do
  name="${NAMES[$index]}"
  letter="$(printf '%s' "ab" | cut -c$((index + 1)))"
  docker run -d --name "$name" --network="container:${PREFIX}-horizon" -v "$HOST_ROOT:/src" -w "$WS" \
    -e "MAX_LOG=debug" rust:1.94.0-bookworm bash -c \
    "timeout 900 $CLIENT_BIN --connect 127.0.0.1:8082 --join $GAME_ID --password '' \
       --player rust${letter^^} --team $index --data-dir $WS/max-original --assets-dir $WS/assets \
       --turns $TURNS > $WS/build/smoke-logs/${PREFIX}-${letter}.log 2>&1" >/dev/null
done

echo "== wait clients =="
for name in "${NAMES[@]}"; do
  docker wait "$name" > "$LOGS/${PREFIX}-${name}.exit" 2>/dev/null || true
done
for index in 0 1; do
  name="${NAMES[$index]}"
  letter="$(printf '%s' "ab" | cut -c$((index + 1)))"
  code="$(tr -d '\r\n' < "$LOGS/${PREFIX}-${name}.exit" 2>/dev/null || echo missing)"
  tail -4 "$LOGS/${PREFIX}-${letter}.log" || true
  [ "$code" = "0" ] || {
    echo "live-smoke-mixed: $name завершился с кодом $code" >&2
    exit 1
  }
done

echo "== checks =="
checksums=()
for index in 0 1; do
  letter="$(printf '%s' "ab" | cut -c$((index + 1)))"
  log="$LOGS/${PREFIX}-${letter}.log"
  grep -q "turns=$TURNS " "$log" || {
    echo "live-smoke-mixed: $log: нет turns=$TURNS" >&2
    exit 1
  }
  grep -q 'barrier_timeouts=0' "$log" || {
    echo "live-smoke-mixed: $log: barrier_timeouts>0" >&2
    exit 1
  }
  ! grep -q 'десинк\|Desync check failed' "$log" || {
    echo "live-smoke-mixed: $log: десинк" >&2
    exit 1
  }
  for barrier in 'барьер 00' 'барьер 52' 'барьер 54' 'барьер 45'; do
    grep -q "$barrier" "$log" || {
      echo "live-smoke-mixed: $log: не найден '$barrier'" >&2
      exit 1
    }
  done
  checksum="$(sed -n 's/.*state_checksum=\(0x[0-9A-Fa-f]*\).*/\1/p' "$log" | tail -1)"
  [ -n "$checksum" ] || {
    echo "live-smoke-mixed: $log: нет state_checksum" >&2
    exit 1
  }
  checksums+=("$checksum")
  echo "  rust-${letter}: checksum=$checksum"
done
[ "${checksums[0]}" = "${checksums[1]}" ] || {
  echo "live-smoke-mixed: CRC Rust×2 не совпал: ${checksums[*]}" >&2
  exit 1
}

docker logs "${PREFIX}-horizon" > "$LOGS/${PREFIX}-horizon.log" 2>&1 || true
rm -f "$LOGS/${PREFIX}-ai_log.txt"
if docker cp "${PREFIX}-horizon:/tmp/horizon-sessions/$GAME_ID/ai_log.txt" \
     "$LOGS/${PREFIX}-ai_log.txt" >/dev/null 2>&1; then
  if grep -q 'Desync check failed' "$LOGS/${PREFIX}-ai_log.txt"; then
    echo "live-smoke-mixed: C++-сервер зафиксировал desync" >&2
    exit 1
  fi
else
  echo "live-smoke-mixed: ai_log.txt не скопирован — C++ desync-детектор не проверен" >&2
  exit 1
fi

if [ -n "$CPP_CLIENT_BLOCKER" ]; then
  echo "live-smoke-mixed: PARTIAL PASS — Rust×2 CRC ${checksums[0]}, C++-сервер без desync"
  echo "  C++ client: BLOCKED — $CPP_CLIENT_BLOCKER"
  echo "  Что доказано: Rust-состояние бит-в-бит совпало с C++-симуляцией на 45 (desync-детектор молчит)."
  echo "  Что не доказано: одновременная сессия Rust-клиента и C++-клиента (нет headless client mode)."
else
  echo "live-smoke-mixed: PASS — Rust×2 CRC ${checksums[0]}, C++-сервер без desync (--client найден, но запуск C++ не автоматизирован)"
fi
