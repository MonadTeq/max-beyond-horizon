#!/usr/bin/env bash
# M6.3: локальный CI (то же, что .github/workflows/ci.yml) в Docker.
#
#   bash ci.sh                 # cargo check/test max-rust + cargo test --workspace Horizon
#                              # + QA: qa_run (sp-full/hs-2/visual-screens/victory) и golden
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

mkdir -p "$ROOT/build/cargo-home"

step() {
    printf '\n===== %s =====\n' "$*"
}

started=$(date +%s)

step "1/3 max-rust: cargo check + test (+ sound feature)"
docker run --rm \
    -v "$ROOT:/src" \
    -v "$ROOT/build/cargo-home:/cargo-home" \
    -e "CARGO_HOME=/cargo-home" \
    -w /src/max-rust \
    "$RUST_IMAGE" bash -c \
    'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libasound2-dev >/dev/null && cargo check && cargo test && cargo check --features sound && cargo test --features sound'

step "2/3 max-horizon-server: cargo test --workspace"
docker run --rm \
    -v "$ROOT/max-horizon-server:/src/max-horizon-server" \
    -w /src/max-horizon-server \
    "$RUST_IMAGE" bash -c \
    'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libssl-dev >/dev/null && cargo test --workspace'

# Слои 2–3 QA (docs/qa-plan.md): скриптовый UI-драйвер без SDL + golden-кадры.
# Каталог вывода пересоздаётся, чтобы старые кадры не маскировали регресс.
step "3/3 QA: qa_run (sp-full, hs-2, visual-screens, victory) + golden"
docker run --rm \
    -v "$ROOT:/src" \
    -v "$ROOT/build/cargo-home:/cargo-home" \
    -e "CARGO_HOME=/cargo-home" \
    -w /src/max-rust \
    "$RUST_IMAGE" bash -c \
    'set -e
     apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null
     rm -rf qa-out
     cargo run -q -p max_app --example qa_run -- qa/sp-full.qa qa-out/sp-full
     cargo run -q -p max_app --example qa_run -- qa/hs-2.qa qa-out/hs-2
     cargo run -q -p max_app --example qa_run -- qa/visual-screens.qa qa-out/visual-screens
     cargo run -q -p max_app --example qa_run -- qa/victory.qa qa-out/victory
     cargo run -q -p max_app --example qa_golden_check -- qa-out/visual-screens qa/golden'

if [ "$SMOKE" = "1" ]; then
    step "SMOKE: build/scripts/live-smoke.sh ($TURNS ходов)"
    bash "$ROOT/build/scripts/live-smoke.sh" "$TURNS"
fi

step "CI OK за $(( $(date +%s) - started )) с"
