#!/usr/bin/env bash
# M6.3: локальный CI (то же, что .github/workflows/ci.yml) в Docker.
#
#   bash ci.sh                 # cargo check/test max-rust + cargo test --workspace Horizon
#   bash ci.sh --smoke         # плюс живой live-smoke (2 клиента × N ходов)
#   bash ci.sh --smoke --turns 10
#
# Требования: Docker с Linux-контейнерами; для --smoke — собранные
# max-horizon-server/target/release/horizon и build/linux-server-template/max-port.
# Запускать из любой оболочки: bash/ Git Bash/ WSL/ Linux.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# Docker Desktop для Windows ждёт путь вида D:/..., а не /d/...
if command -v cygpath >/dev/null 2>&1; then
    ROOT="$(cygpath -m "$ROOT")"
fi

RUST_IMAGE="${RUST_IMAGE:-rust:1.94.0-bookworm}"
SMOKE=0
TURNS="${SMOKE_TURNS:-5}"

usage() {
    cat <<'EOF'
Использование: bash ci.sh [--smoke] [--turns N]

  --smoke       после тестов запустить build/scripts/live-smoke.sh (2 клиента)
  --turns N     число ходов для --smoke (по умолчанию 5)
  -h, --help    эта справка

Переменные: RUST_IMAGE (rust:1.94.0-bookworm), SMOKE_TURNS.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --smoke)
            SMOKE=1
            shift
            ;;
        --turns)
            [ "$#" -ge 2 ] || {
                echo "ci.sh: --turns требует число" >&2
                exit 2
            }
            TURNS="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "ci.sh: неизвестный аргумент: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

command -v docker >/dev/null 2>&1 || {
    echo "ci.sh: docker не найден в PATH" >&2
    exit 1
}

step() {
    printf '\n===== %s =====\n' "$*"
}

started=$(date +%s)

step "1/2 max-rust: cargo check + cargo test"
docker run --rm \
    -v "$ROOT:/src" \
    -w /src/max-rust \
    "$RUST_IMAGE" bash -c \
    'apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null && cargo check && cargo test'

step "2/2 max-horizon-server: cargo test --workspace"
docker run --rm \
    -v "$ROOT/max-horizon-server:/src/max-horizon-server" \
    -w /src/max-horizon-server \
    "$RUST_IMAGE" bash -c \
    'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libssl-dev >/dev/null && cargo test --workspace'

if [ "$SMOKE" = "1" ]; then
    step "SMOKE: build/scripts/live-smoke.sh ($TURNS ходов)"
    bash "$ROOT/build/scripts/live-smoke.sh" "$TURNS"
fi

step "CI OK за $(( $(date +%s) - started )) с"
