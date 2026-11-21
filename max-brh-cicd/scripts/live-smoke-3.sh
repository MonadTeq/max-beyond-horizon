#!/usr/bin/env bash
# Живой smoke P5: Horizon (WS+gRPC) + C++-headless + ТРИ Rust-клиента × N ходов.
#
#   bash max-brh-cicd/scripts/live-smoke-3.sh [turns]
#
# Проверяет то, чего не проверял двухклиентный live-smoke.sh:
#   * третий клиент занимает BLUE (--min-players 3), а не компьютерный слот;
#   * у каждого клиента ровно два пира и все барьеры 00/52/54/45 проходят;
#   * релей доходит до всех: каждый клиент видит 00 команд двух других;
#   * state_checksum совпадает у всех трёх (CRC-барьер 45 сошёлся);
#   * barrier_timeouts=0, в логах нет десинка/ошибок сессии.
#
# Требования: Docker; workspace смонтирован как /src; собраны
#   max-brh-server/target/release/horizon и build/linux-server-template/max-port.
# Клиенты собираются один раз (--no-default-features) и запускаются прямо из
# target/debug, поэтому три параллельных `cargo run` не конкурируют за lock.
set -euo pipefail

TURNS="${1:-5}"
CLIENTS=3
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
PREFIX="live3"
NAMES=("${PREFIX}-a" "${PREFIX}-b" "${PREFIX}-c")
TEAMS=(0 1 2)

cleanup() {
  docker rm -f "${PREFIX}-horizon" "${NAMES[@]}" >/dev/null 2>&1 || true
}
trap cleanup EXIT

cleanup
mkdir -p "$LOGS"
: > "$LOGS/${PREFIX}-a.log"
: > "$LOGS/${PREFIX}-b.log"
: > "$LOGS/${PREFIX}-c.log"

# 0. Один раз собираем клиент и gRPC-пробу без SDL (headless --connect).
echo "== build: max_app + horizon_probe (no-default-features) =="
docker run --rm -v "$HOST_ROOT:/src" -w "$WS/max-brh" rust:1.94.0-bookworm bash -c \
  "apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && \
   cargo build -q --locked --no-default-features -p max_app --example horizon_probe"
CLIENT_BIN="$WS/max-brh/target/debug/max-brh"
PROBE_BIN="$WS/max-brh/target/debug/examples/horizon_probe"

# 1. Horizon. Порты наружу не публикуем: probe и клиенты живут в его network
#    namespace, поэтому занятые хостом 8080/8082 не мешают прогону.
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
  echo "live-smoke-3: Horizon не запустился" >&2
  docker logs "${PREFIX}-horizon" >&2 || true
  exit 1
fi

# 2. Создать игру на 3 игрока, забрать id. Проба сразу выходит, слот не занимает.
echo "== create game (min_players=$CLIENTS) =="
docker run --rm --network="container:${PREFIX}-horizon" -v "$HOST_ROOT:/src" -w "$WS" \
  rust:1.94.0-bookworm bash -c \
  "$PROBE_BIN 127.0.0.1 8082 smoke $CLIENTS 2>/dev/null | grep create_game | sed 's/.*id=//;s/ name=.*//'" \
  > "$LOGS/${PREFIX}-game_id.txt" || true
GAME_ID="$(tr -d '\r\n' < "$LOGS/${PREFIX}-game_id.txt")"
if [ -z "$GAME_ID" ]; then
  echo "live-smoke-3: horizon_probe не вернул create_game (см. выше)" >&2
  exit 1
fi
echo "GAME_ID=$GAME_ID"

# 3. Три клиента одновременно: сессия стартует только после третьего READY,
#    поэтому BLUE гарантированно занимает реальный клиент, а не компьютер.
#    MAX_LOG=debug нужен для проверки релея (00 от двух других команд) и
#    прохождения 52/54/45.
for index in 0 1 2; do
  name="${NAMES[$index]}"
  team="${TEAMS[$index]}"
  letter="$(printf '%s' "abc" | cut -c$((index + 1)))"
  docker run -d --name "$name" --network="container:${PREFIX}-horizon" -v "$HOST_ROOT:/src" -w "$WS" \
    -e "MAX_LOG=debug" rust:1.94.0-bookworm bash -c \
    "timeout 900 $CLIENT_BIN --connect 127.0.0.1:8082 --join $GAME_ID --password '' \
       --player rust${letter^^} --team $team --data-dir $WS/max-original --assets-dir $WS/assets \
       --turns $TURNS > $WS/build/smoke-logs/${PREFIX}-${letter}.log 2>&1" >/dev/null
done

# 4. Ждём все три процесса и сохраняем коды выхода.
echo "== wait clients (max ~15 мин) =="
for name in "${NAMES[@]}"; do
  docker wait "$name" > "$LOGS/${PREFIX}-${name}.exit" 2>/dev/null || true
done

for index in 0 1 2; do
  name="${NAMES[$index]}"
  code="$(tr -d '\r\n' < "$LOGS/${PREFIX}-${name}.exit" 2>/dev/null || echo missing)"
  echo "== ${name} exit=${code} =="
  tail -8 "$LOGS/${PREFIX}-$(printf '%s' 'abc' | cut -c$((index + 1))).log" || true
  if [ "$code" != "0" ]; then
    echo "live-smoke-3: $name завершился с кодом $code" >&2
    exit 1
  fi
done

# 5. Проверки. Порядок как в live-smoke.sh: сначала грубые ошибки, затем
#    инварианты трёхклиентной сессии.
echo "== checks =="
state_checksums=()
for index in 0 1 2; do
  name="${NAMES[$index]}"
  letter="$(printf '%s' "abc" | cut -c$((index + 1)))"
  log="$LOGS/${PREFIX}-${letter}.log"

  if grep -q 'Session failed\|сессия упала\|SessionError' "$log"; then
    echo "live-smoke-3: $log: ошибка сессии" >&2
    exit 1
  fi
  if grep -q 'barrier_timeouts=[1-9]' "$log"; then
    echo "live-smoke-3: $log: барьерный таймаут (barrier_timeouts>0)" >&2
    exit 1
  fi
  if grep -q 'десинк\|Desync check failed' "$log"; then
    echo "live-smoke-3: $log: десинк" >&2
    exit 1
  fi
  if ! grep -q "turns=$TURNS " "$log"; then
    echo "live-smoke-3: $log: нет turns=$TURNS в итоговом отчёте" >&2
    grep '^network:' "$log" >&2 || true
    exit 1
  fi
  if ! grep -q 'пиров=2' "$log"; then
    echo "live-smoke-3: $log: клиент видит не двух пиров" >&2
    exit 1
  fi

  # Каждый барьер реально пройден (52/54/45 — debug-строки).
  for barrier in 'барьер 00' 'барьер 52' 'барьер 54' 'барьер 45'; do
    if ! grep -q "$barrier" "$log"; then
      echo "live-smoke-3: $log: не найден проход '$barrier'" >&2
      exit 1
    fi
  done

  # Релей: клиент видит 00 от каждой из двух других команд (team из payload).
  for other in 0 1 2; do
    [ "$other" = "$index" ] && continue
    if ! grep -q "00: команда $other " "$log"; then
      echo "live-smoke-3: $log: не видит релейный 00 команды $other" >&2
      exit 1
    fi
  done

  checksum="$(sed -n 's/.*state_checksum=\(0x[0-9A-Fa-f]*\).*/\1/p' "$log" | tail -1)"
  if [ -z "$checksum" ]; then
    echo "live-smoke-3: $log: нет state_checksum" >&2
    exit 1
  fi
  state_checksums+=("$checksum")
  echo "  $name: team=$index checksum=$checksum"
done

if [ "${state_checksums[0]}" != "${state_checksums[1]}" ] || \
   [ "${state_checksums[0]}" != "${state_checksums[2]}" ]; then
  echo "live-smoke-3: state_checksum не совпал: ${state_checksums[*]}" >&2
  exit 1
fi

# 6. Серверные логи сессии: нет desync, все три команды дошли до 45.
docker logs "${PREFIX}-horizon" > "$LOGS/${PREFIX}-horizon.log" 2>&1 || true
if grep -q 'Desync check failed' "$LOGS/${PREFIX}-horizon.log"; then
  echo "live-smoke-3: сервер зафиксировал desync" >&2
  exit 1
fi
rm -f "$LOGS/${PREFIX}-ai_log.txt" "$LOGS/${PREFIX}-session_stderr.txt"
if docker cp "${PREFIX}-horizon:/tmp/horizon-sessions/$GAME_ID/ai_log.txt" \
     "$LOGS/${PREFIX}-ai_log.txt" >/dev/null 2>&1; then
  if grep -q 'Desync check failed' "$LOGS/${PREFIX}-ai_log.txt"; then
    echo "live-smoke-3: ai_log сессии зафиксировал desync" >&2
    exit 1
  fi
else
  echo "live-smoke-3: ai_log.txt сессии не скопирован — серверный desync-детектор не проверен" >&2
  exit 1
fi
docker cp "${PREFIX}-horizon:/tmp/horizon-sessions/$GAME_ID/session_stderr.txt" \
  "$LOGS/${PREFIX}-session_stderr.txt" >/dev/null 2>&1 || true

echo "live-smoke-3: PASS — 3 клиента × $TURNS ходов, релей всем, 00/52/54/45, CRC ${state_checksums[0]}"
