#!/usr/bin/env bash
# M6.3: локальный CI (то же, что .github/workflows/ci.yml) в Docker.
#
#   bash ci.sh                 # cargo check/test max-brh + cargo test --workspace Horizon
#                              # + QA: qa_run (sp-full/…/soak-100/edge-cases) и golden
#                              # + SOAK: soak_headless 120 ходов
#   bash ci.sh --smoke         # плюс живой live-smoke (2 клиента × N ходов)
#   bash ci.sh --smoke3        # плюс live-smoke-3 (3 клиента × N ходов)
#   bash ci.sh --smoke-mixed   # плюс live-smoke-mixed (Rust×2 + C++ desync-оракул)
#   bash ci.sh --smoke --turns 10
#
# Требования: Docker с Linux-контейнерами; для --smoke — собранные
# max-horizon-server/target/release/horizon и build/linux-server-template/max-port.
# Запускать из любой оболочки: bash/ Git Bash/ WSL/ Linux.
set -euo pipefail

# Git Bash (MSYS2/Cygwin) переписывает POSIX-пути в аргументах прямо в
# Windows-пути: `docker run -w /src/max-brh` превращался в
# `-w C:/Program Files/Git/src/max-brh` и docker падал с «working directory is
# invalid». Пути внутри контейнера обязаны остаться POSIX — конверсию отключаем
# (на Linux/macOS эти переменные игнорируются).
if [ -n "${MSYSTEM:-}" ] || command -v cygpath >/dev/null 2>&1; then
    export MSYS_NO_PATHCONV=1
    export MSYS2_ARG_CONV_EXCL='*'
fi

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# Docker Desktop для Windows ждёт путь вида D:/..., а не /d/...
if command -v cygpath >/dev/null 2>&1; then
    ROOT="$(cygpath -m "$ROOT")"
fi

RUST_IMAGE="${RUST_IMAGE:-rust:1.94.0-bookworm}"
SMOKE=0
SMOKE3=0
SMOKE_MIXED=0
TURNS="${SMOKE_TURNS:-5}"

usage() {
    cat <<'EOF'
Использование: bash ci.sh [--smoke] [--smoke3] [--smoke-mixed] [--turns N]

  --smoke         после тестов запустить build/scripts/live-smoke.sh (2 клиента)
  --smoke3        после тестов запустить build/scripts/live-smoke-3.sh (3 клиента)
  --smoke-mixed   после тестов запустить build/scripts/live-smoke-mixed.sh
                  (Rust×2 + C++ desync-оракул; при отсутствии C++ headless
                  client mode — PARTIAL PASS)
  --turns N       число ходов для smoke-скриптов (по умолчанию 5)
  -h, --help      эта справка

Переменные: RUST_IMAGE (rust:1.94.0-bookworm), SMOKE_TURNS.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --smoke)
            SMOKE=1
            shift
            ;;
        --smoke3)
            SMOKE3=1
            shift
            ;;
        --smoke-mixed)
            SMOKE_MIXED=1
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

step "1/4 max-brh: cargo check + test (default = GUI/SDL from source; + sound; + headless)"
# default max_app = ["sdl-from-source"]: SDL3 3.4.20 собирается из исходников
# через CMake (vendored sdl3-src) — системный SDL3/vcpkg не нужен. На Linux
# CMake собирает SDL3 с теми драйверами, чьи dev-заголовки видит, поэтому ставим
# полный набор из max-brh-docs/development/release-checklist.md §2.3: X11
# (x11/xext/xrender/xcursor/xi/xrandr/xinerama/xss/xtst/xfixes), Wayland
# (wayland/xkbcommon/libdecor), EGL/GLES/GL, KMSDRM (drm/gbm), ALSA (звук SDL и
# feature sound), dbus-1 и libudev. Без них SDL3 соберётся с dummy-драйверами —
# окно на Linux не откроется.
# libSDL3.so.0 после сборки лежит в target/debug (sdl3-sys копирует её из
# CMake-каталога) и линкуется динамически — тестовым бинарям нужен
# LD_LIBRARY_PATH на target/debug (в артефактах это делает run.sh).
SDL_DEPS='gcc g++ cmake pkg-config libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev libxi-dev libxrandr-dev libxinerama-dev libxss-dev libxtst-dev libxfixes-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols libdecor-0-dev libegl-dev libgles-dev libgl-dev libgbm-dev libdrm-dev libdbus-1-dev libudev-dev'
docker run --rm \
    -v "$ROOT:/src" \
    -v "$ROOT/build/cargo-home:/cargo-home" \
    -e "CARGO_HOME=/cargo-home" \
    -w /src/max-brh \
    "$RUST_IMAGE" bash -c \
    "apt-get update -qq && apt-get install -y -qq $SDL_DEPS >/dev/null && \
     export LD_LIBRARY_PATH=/src/max-brh/target/debug\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH} && \
     cargo check && cargo test && \
     cargo check --features sound && cargo test --features sound && \
     cargo test --no-default-features"

step "2/4 max-horizon-server: cargo test --workspace"
docker run --rm \
    -v "$ROOT/max-horizon-server:/src/max-horizon-server" \
    -w /src/max-horizon-server \
    "$RUST_IMAGE" bash -c \
    'apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libssl-dev >/dev/null && cargo test --workspace'

# Слои 2–3 QA (max-brh-qa/plans/qa-plan.md): скриптовый UI-драйвер без SDL + golden-кадры.
# Каталог вывода пересоздаётся, чтобы старые кадры не маскировали регресс.
# Примеры собираются с --no-default-features: QA-драйвер in-process, окно ему не
# нужно, а default GUI потянул бы CMake-сборку SDL3 и X11-заголовки.
# Сценарии и эталоны лежат в ../max-brh-qa/ (рабочий каталог — /src/max-brh).
step "3/4 QA: qa_run (sp-full, hs-2, visual-screens, victory, production, save-load, upgrade, research, edge-cases, cargo-full, move-input, research-help, unit-menu, allocate, preferences, xfer, ai-turn, sp-tb, combat-tb, missile-line, build-cancel, panels) + golden"
docker run --rm \
    -v "$ROOT:/src" \
    -v "$ROOT/build/cargo-home:/cargo-home" \
    -e "CARGO_HOME=/cargo-home" \
    -w /src/max-brh \
    "$RUST_IMAGE" bash -c \
    'set -e
     apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null
     rm -rf qa-out
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-full.qa qa-out/sp-full
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/hs-2.qa qa-out/hs-2
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/visual-screens.qa qa-out/visual-screens
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/victory.qa qa-out/victory
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/production.qa qa-out/production
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/save-load.qa qa-out/save-load
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/upgrade.qa qa-out/upgrade
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/research.qa qa-out/research
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/edge-cases.qa qa-out/edge-cases
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/cargo-full.qa qa-out/cargo-full
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/move-input.qa qa-out/move-input
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/research-help.qa qa-out/research-help
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/unit-menu.qa qa-out/unit-menu
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/allocate.qa qa-out/allocate
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/preferences.qa qa-out/preferences
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/xfer.qa qa-out/xfer
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/ai-turn.qa qa-out/ai-turn
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/sp-tb.qa qa-out/sp-tb
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/combat-tb.qa qa-out/combat-tb
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/missile-line.qa qa-out/missile-line
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/build-cancel.qa qa-out/build-cancel
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/panels.qa qa-out/panels
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/visual-screens ../max-brh-qa/golden
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/production ../max-brh-qa/golden/production
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/upgrade ../max-brh-qa/golden/upgrade
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/research ../max-brh-qa/golden/research
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/cargo-full ../max-brh-qa/golden/cargo
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/move-input ../max-brh-qa/golden/move-input
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/unit-menu ../max-brh-qa/golden/unit-menu
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/allocate ../max-brh-qa/golden/allocate
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/preferences ../max-brh-qa/golden/preferences
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/xfer ../max-brh-qa/golden/xfer
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/ai-turn ../max-brh-qa/golden/ai-turn
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/sp-tb ../max-brh-qa/golden/sp-tb
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/combat-tb ../max-brh-qa/golden/combat-tb
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/missile-line ../max-brh-qa/golden/missile-line
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/build-cancel ../max-brh-qa/golden/build-cancel
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/panels ../max-brh-qa/golden/panels
     cargo run -q -p max_app --no-default-features --example qa_golden_check -- qa-out/research-help ../max-brh-qa/golden/research-help'

# P4: soak — отдельная стадия (не быстрая QA): 100 ходов UI-партии с ходами ИИ
# + 120 ходов headless ИИ×ИИ с профилем тика (MAX_TICK_PROFILE=1) и RSS.
# Каталог qa-out/ уже создан стадией 3/4; soak-логи дописываются рядом.
step "4/4 SOAK: soak-100.qa (100 ходов, память+профиль) + soak_headless (120 ходов)"
docker run --rm \
    -v "$ROOT:/src" \
    -v "$ROOT/build/cargo-home:/cargo-home" \
    -e "CARGO_HOME=/cargo-home" \
    -e "MAX_TICK_PROFILE=1" \
    -w /src/max-brh \
    "$RUST_IMAGE" bash -c \
    'set -e
     apt-get update -qq && apt-get install -y -qq gcc g++ cmake >/dev/null
     mkdir -p qa-out/soak-100 qa-out/soak-headless
     cargo run -q -p max_app --no-default-features --example qa_run -- ../max-brh-qa/scripts/soak-100.qa qa-out/soak-100
     cargo run -q -p max_app --no-default-features --example soak_headless -- --turns 120 --mode tb --out qa-out/soak-headless'

if [ "$SMOKE" = "1" ]; then
    step "SMOKE: build/scripts/live-smoke.sh ($TURNS ходов)"
    bash "$ROOT/build/scripts/live-smoke.sh" "$TURNS"
fi

if [ "$SMOKE3" = "1" ]; then
    step "SMOKE3: build/scripts/live-smoke-3.sh ($TURNS ходов, 3 клиента)"
    bash "$ROOT/build/scripts/live-smoke-3.sh" "$TURNS"
fi

if [ "$SMOKE_MIXED" = "1" ]; then
    step "SMOKE-MIXED: build/scripts/live-smoke-mixed.sh ($TURNS ходов)"
    bash "$ROOT/build/scripts/live-smoke-mixed.sh" "$TURNS"
fi

step "CI OK за $(( $(date +%s) - started )) с"

# Очистка мусора после прогонов (остановленные контейнеры, dangling-образы,
# build-cache). Отключается: DOCKER_CLEAN=0 bash ci.sh
if [ "${DOCKER_CLEAN:-1}" = "1" ]; then
    step "DOCKER-CLEAN: build/scripts/docker-clean.sh"
    bash "$ROOT/build/scripts/docker-clean.sh"
fi
