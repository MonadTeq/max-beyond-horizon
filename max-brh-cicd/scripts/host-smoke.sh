#!/usr/bin/env bash
# Native P2P host smoke — `PLAN-host-mode.md` §5 H5, gate (е).
#
# Spawns native headless `max-brh` processes on 127.0.0.1 — one `--host` and
# `clients - 1` direct `--connect` guests — and plays `--turns` turns of the P2P
# host mode. The gate asserts:
#   * every client finished the requested number of turns;
#   * `desyncs=0` and `barrier_timeouts=0` on every client;
#   * the shared digest (`shared:` line) is identical on every client;
#   * no `desync-turn*.txt` report and no `host-audit:`/`audit:` line was
#     produced (the desync audit 23/24/49 never had to run).
#
# No Horizon, no Docker, no `live-smoke.sh`, no C++ oracle: this is the pure
# Rust native star topology (host + direct guests).
#
# Usage:
#   bash max-brh-cicd/scripts/host-smoke.sh [clients] [turns]
#   bash max-brh-cicd/scripts/host-smoke.sh all [turns]    # 2 and 3 clients
#
#   clients  2 (default) or 3
#   turns    number of turns, default 10
#
# Environment:
#   HOST_SMOKE_BIN      reuse an already built `max-brh` binary
#   HOST_SMOKE_PROFILE  debug (default) | release
#   HOST_SMOKE_PORT     base UDP port (default: a random 32000..34999)
#   HOST_SMOKE_TIMEOUT  seconds to wait for the match (default 240)
#   HOST_SMOKE_LOGS     log directory (default <repo>/build/host-smoke)
#   HOST_SMOKE_DATA     game data directory (default <repo>/max-original)
#   HOST_SMOKE_WORLD    planet index (default 12 — GREEN_1)
#   HOST_SMOKE_SEED     RNG seed of the lobby settings (default 4242)
#   CARGO_TARGET_DIR    respected by the build
set -u

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
# Git Bash / MSYS: keep the Windows-style paths we pass to native processes.
if command -v cygpath >/dev/null 2>&1; then
  ROOT="$(cygpath -m "$ROOT")"
fi

DATA_DIR="${HOST_SMOKE_DATA:-$ROOT/max-original}"
LOGS="${HOST_SMOKE_LOGS:-$ROOT/build/host-smoke}"
TIMEOUT="${HOST_SMOKE_TIMEOUT:-240}"
PROFILE="${HOST_SMOKE_PROFILE:-debug}"
WORLD="${HOST_SMOKE_WORLD:-12}"
SEED="${HOST_SMOKE_SEED:-4242}"
BASE_PORT="${HOST_SMOKE_PORT:-}"

usage() {
  sed -n '2,30p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

case "${1:-}" in
  -h|--help|help)
    usage
    exit 0
    ;;
esac

if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
  PATH="$HOME/.cargo/bin:$PATH"
  export PATH
fi

# --- build (unless an explicit binary was given) --------------------------
mkdir -p "$LOGS"
if [ -n "${HOST_SMOKE_BIN:-}" ]; then
  BIN="$HOST_SMOKE_BIN"
else
  PROFILE_FLAG=""
  if [ "$PROFILE" = "release" ]; then
    PROFILE_FLAG="--release"
  fi
  echo "host-smoke: cargo build --no-default-features -p max_app $PROFILE_FLAG"
  # Build from the workspace directory: `max-brh/.cargo/config.toml` raises the
  # Windows main-thread stack to 8 MiB (the headless debug build overflows the
  # default 1 MiB), and cargo only reads `.cargo/config.toml` from the current
  # directory ancestry.
  if ! ( cd "$ROOT/max-brh" && cargo build --no-default-features -p max_app $PROFILE_FLAG ) \
      >"$LOGS/build.log" 2>&1; then
    echo "host-smoke: сборка не удалась (см. $LOGS/build.log)" >&2
    tail -20 "$LOGS/build.log" >&2 || true
    exit 1
  fi
  TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/max-brh/target}"
  BIN="$TARGET_DIR/$PROFILE/max-brh"
  [ -x "$BIN" ] || BIN="$BIN.exe"
  if [ ! -x "$BIN" ]; then
    echo "host-smoke: собранный бинарь не найден: $BIN" >&2
    exit 1
  fi
fi
BIN="$(cd -- "$(dirname -- "$BIN")" && pwd)/$(basename -- "$BIN")"

if [ ! -f "$DATA_DIR/MAX.RES" ]; then
  echo "host-smoke: нет данных игры: $DATA_DIR/MAX.RES (нужен исходный M.A.X.)" >&2
  exit 1
fi


# --- helpers ---------------------------------------------------------------
PIDS=()
cleanup() {
  for pid in "${PIDS[@]:-}"; do
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
    fi
  done
}
trap cleanup EXIT INT TERM

pick_port() {
  if [ -n "$BASE_PORT" ]; then
    echo "$BASE_PORT"
    return
  fi
  echo $((32000 + RANDOM % 2000))
}

# Field of a `key=value` token in `line` (e.g. `turns=10`).
field() {
  # $1 — key, $2 — line
  printf '%s\n' "$2" | sed -n "s/.*[[:space:]]$1=\([^[:space:]]*\).*/\1/p" | head -n 1
}

# Runs `clients` clients for `turns` turns; returns non-zero on a failed gate.
run_case() {
  local clients="$1" turns="$2"
  local label="${clients}c"
  local dir="$LOGS/${label}-${turns}turns"
  rm -rf "$dir"
  mkdir -p "$dir"

  local port host_pid attempt i pid ok=0
  for attempt in 1 2 3 4 5; do
    port=$(pick_port)
    local host_dir="$dir/host"
    mkdir -p "$host_dir"
    (
      cd "$host_dir" || exit 1
      exec "$BIN" --host --listen 127.0.0.1 --host-port "$port" \
        --min-players "$clients" --turns "$turns" --player host --team 0 \
        --world "$WORLD" --seed "$SEED" --data-dir "$DATA_DIR"
    ) >"$host_dir/out.log" 2>&1 &
    host_pid=$!
    PIDS+=("$host_pid")
    sleep 1
    if kill -0 "$host_pid" 2>/dev/null; then
      ok=1
      break
    fi
    echo "host-smoke: порт $port занят, повтор (попытка $attempt/5)"
  done
  if [ "$ok" -ne 1 ]; then
    echo "host-smoke: не удалось поднять хост (порты заняты)" >&2
    return 1
  fi
  echo "host-smoke: $clients клиент(ов), $turns ходов, 127.0.0.1:$port"

  for i in $(seq 1 $((clients - 1))); do
    local guest_dir="$dir/guest$i"
    mkdir -p "$guest_dir"
    local team="$i"
    (
      cd "$guest_dir" || exit 1
      exec "$BIN" --connect "127.0.0.1:$port" --player "guest$i" --team "$team" \
        --turns "$turns" --data-dir "$DATA_DIR"
    ) >"$guest_dir/out.log" 2>&1 &
    pid=$!
    PIDS+=("$pid")
    sleep 0.3
  done

  # Wait for every process with a wall-clock bound.
  local waited=0 alive
  while [ "$waited" -lt "$TIMEOUT" ]; do
    alive=0
    for pid in "${PIDS[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        alive=1
      fi
    done
    [ "$alive" -eq 0 ] && break
    sleep 1
    waited=$((waited + 1))
  done
  if [ "$alive" -ne 0 ]; then
    echo "host-smoke: матч не завершился за ${TIMEOUT}s" >&2
    cleanup
    return 1
  fi
  # Give the ENet teardown a moment to flush the logs.
  sleep 0.3

  # --- assertions ----------------------------------------------------------
  local host_line guest_line shared_ref shared_line log failed=0
  host_line=$(grep -m 1 '^host-game: ' "$dir/host/out.log" || true)
  if [ -z "$host_line" ]; then
    echo "host-smoke: в логе хоста нет строки host-game:" >&2
    tail -20 "$dir/host/out.log" >&2 || true
    failed=1
  else
    local host_turns host_desyncs host_timeouts
    host_turns=$(field turns "$host_line")
    host_desyncs=$(field desyncs "$host_line")
    host_timeouts=$(field barrier_timeouts "$host_line")
    if [ "$host_turns" != "$turns" ] || [ "$host_desyncs" != "0" ] || [ "$host_timeouts" != "0" ]; then
      echo "host-smoke: FAIL host: turns=$host_turns/$turns desyncs=$host_desyncs barrier_timeouts=$host_timeouts" >&2
      failed=1
    fi
    shared_ref=$(sed -n 's/^shared: //p' "$dir/host/out.log" | head -n 1)
    if [ -z "$shared_ref" ]; then
      echo "host-smoke: в логе хоста нет shared:" >&2
      failed=1
    fi
  fi

  for i in $(seq 1 $((clients - 1))); do
    log="$dir/guest$i/out.log"
    guest_line=$(grep -m 1 '^network: ' "$log" || true)
    if [ -z "$guest_line" ]; then
      echo "host-smoke: в логе guest$i нет строки network:" >&2
      tail -20 "$log" >&2 || true
      failed=1
      continue
    fi
    local g_turns g_desyncs g_timeouts g_shared
    g_turns=$(field turns "$guest_line")
    g_desyncs=$(field desyncs "$guest_line")
    g_timeouts=$(field barrier_timeouts "$guest_line")
    g_shared=$(sed -n 's/^shared: //p' "$log" | head -n 1)
    if [ "$g_turns" != "$turns" ] || [ "$g_desyncs" != "0" ] || [ "$g_timeouts" != "0" ]; then
      echo "host-smoke: FAIL guest$i: turns=$g_turns/$turns desyncs=$g_desyncs barrier_timeouts=$g_timeouts" >&2
      failed=1
    fi
    if [ -n "$shared_ref" ] && [ "$g_shared" != "$shared_ref" ]; then
      echo "host-smoke: FAIL guest$i: shared digest расходится" >&2
      echo "  host:  $shared_ref" >&2
      echo "  guest: $g_shared" >&2
      failed=1
    fi
  done

  # No desync audit may have run: no report file, no audit line.
  local reports
  reports=$(find "$dir" -name 'desync-turn*.txt' -print -quit)
  if [ -n "$reports" ]; then
    echo "host-smoke: FAIL создан отчёт десинк-аудита: $reports" >&2
    failed=1
  fi
  if grep -qE '^(host-)?audit: ' "$dir"/host/out.log "$dir"/guest*/out.log 2>/dev/null; then
    echo "host-smoke: FAIL в логах есть строка десинк-аудита" >&2
    failed=1
  fi

  if [ "$failed" -ne 0 ]; then
    echo "host-smoke: FAIL ($clients клиент(ов), $turns ходов) — логи: $dir" >&2
    return 1
  fi

  echo "HOST-SMOKE PASS clients=$clients turns=$turns"
  echo "  shared: $shared_ref"
  echo "  host: $host_line"
  for i in $(seq 1 $((clients - 1))); do
    echo "  guest$i: $(grep -m 1 '^network: ' "$dir/guest$i/out.log")"
  done
  return 0
}

# --- arguments ---------------------------------------------------------------
FIRST="${1:-all}"
TURNS="${2:-10}"
case "$FIRST" in
  all|both)
    CASES="2 3"
    ;;
  2|3)
    CASES="$FIRST"
    ;;
  *)
    echo "host-smoke: ожидалось 2|3|all, получено '$FIRST'" >&2
    usage >&2
    exit 2
    ;;
esac
if ! printf '%s' "$TURNS" | grep -qE '^[0-9]+$' || [ "$TURNS" -lt 1 ]; then
  echo "host-smoke: turns должно быть положительным целым, получено '$TURNS'" >&2
  exit 2
fi

FAILED=0
for case_clients in $CASES; do
  PIDS=()
  if ! run_case "$case_clients" "$TURNS"; then
    FAILED=1
  fi
done
if [ "$FAILED" -ne 0 ]; then
  echo "host-smoke: FAILED" >&2
  exit 1
fi
echo "host-smoke: OK ($CASES клиент(ов), $TURNS ходов)"
