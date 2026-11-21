#!/usr/bin/env bash
# Кросс-сборка Windows-бинарника M.A.X. Rust-клиента (max_app) в Docker.
#
#   bash max-brh-cicd/scripts/build-windows.sh             # сборка + упаковка + wine-smoke
#   bash max-brh-cicd/scripts/build-windows.sh --no-wine   # без wine (быстрее, без ~400 МБ apt)
#   bash max-brh-cicd/scripts/build-windows.sh --dry-run   # показать план, ничего не делать
#   bash max-brh-cicd/scripts/build-windows.sh --out /tmp/rel
#
# Что происходит: в контейнере `rust:1.94.0-bookworm` ставятся
# gcc/binutils-mingw-w64-x86-64 (для enet — C-библиотеки ENet 1.3.18), `file`,
# `zip` и (опционально) `wine`, добавляется target `x86_64-pc-windows-gnu`, затем:
#
#   cargo build --release --locked --target x86_64-pc-windows-gnu \
#               -p max_app --no-default-features
#
# Headless: фичи `sdl*` (SDL3 из исходников/vcpkg) и `sound` (rodio/cpal →
# ALSA/WASAPI) не включаются — им нужен Windows-хост с CMake/MSVC/vcpkg
# (см. README.txt в архиве и max-brh-docs/development/release-checklist.md §6).
#
# Артефакты (в $OUT, по умолчанию <repo>/bin/release):
#   max_app-<version>-windows-x86_64/            каталог: max-brh.exe + README.txt +
#                                                LICENSE + CHANGELOG.md + SHA256SUMS
#   max_app-<version>-windows-x86_64.zip         архив (zip из контейнера)
#   max_app-<version>-windows-x86_64.zip.sha256  sha256 архива
#
# Переменные: RUST_IMAGE (rust:1.94.0-bookworm), OUT, WINE_STRICT=1 — падать,
# если wine-smoke не прошёл (по умолчанию это предупреждение, артефакты готовы).
set -euo pipefail

# Git Bash (MSYS2/Cygwin) переписывает POSIX-пути в Windows-пути прямо в
# аргументах команд: `docker run -w /src/max-brh` превращался в
# `-w C:/Program Files/Git/src/max-brh` и docker падал с «working directory
# is invalid». Пути внутри контейнера обязаны остаться POSIX-путями, поэтому
# конверсию аргументов отключаем (на Linux/macOS эти переменные игнорируются).
if [ -n "${MSYSTEM:-}" ] || command -v cygpath >/dev/null 2>&1; then
    export MSYS_NO_PATHCONV=1
    export MSYS2_ARG_CONV_EXCL='*'
fi

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
# Docker Desktop для Windows ждёт D:/..., а не /d/...
if command -v cygpath >/dev/null 2>&1; then
    ROOT="$(cygpath -m "$ROOT")"
fi

RUST_IMAGE="${RUST_IMAGE:-rust:1.94.0-bookworm}"
OUT="${OUT:-$ROOT/bin/release}"
TARGET="x86_64-pc-windows-gnu"
DO_WINE=1
DRY_RUN=0

usage() {
    cat <<'EOF'
Использование: bash max-brh-cicd/scripts/build-windows.sh [--dry-run] [--no-wine] [--out DIR]

  --dry-run     показать план сборки/упаковки и выйти (Docker не запускается)
  --no-wine     не ставить wine и не делать wine-smoke (--help + --demo 1 ход)
  --out DIR     каталог артефактов (по умолчанию <repo>/bin/release)
  -h, --help    эта справка

Переменные: RUST_IMAGE (rust:1.94.0-bookworm), OUT, WINE_STRICT=1.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run) DRY_RUN=1; shift ;;
        --no-wine) DO_WINE=0; shift ;;
        --out)
            [ "$#" -ge 2 ] || { echo "build-windows.sh: --out требует каталог" >&2; exit 2; }
            OUT="$2"
            shift 2
            ;;
        -h|--help) usage; exit 0 ;;
        *) echo "build-windows.sh: неизвестный аргумент: $1" >&2; usage >&2; exit 2 ;;
    esac
done

MANIFEST="$ROOT/max-brh/Cargo.toml"
[ -f "$MANIFEST" ] || { echo "build-windows.sh: нет $MANIFEST" >&2; exit 1; }

VERSION="$(awk -F'"' '/^version = / { print $2; exit }' "$MANIFEST")"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "build-windows.sh: не удалось прочитать версию из $MANIFEST (получено '$VERSION')" >&2
    exit 1
fi

BUNDLE_NAME="max_app-${VERSION}-windows-x86_64"
mkdir -p "$OUT"
OUT="$(cd -- "$OUT" && pwd)"
if command -v cygpath >/dev/null 2>&1; then
    OUT_HOST="$(cygpath -m "$OUT")"
else
    OUT_HOST="$OUT"
fi
BUNDLE="$OUT/$BUNDLE_NAME"
ZIP="$OUT/$BUNDLE_NAME.zip"
CARGO_HOME_DIR="$ROOT/build/cargo-home"
SMOKE_LOG="$ROOT/build/smoke-logs/windows-wine-smoke.log"

for required in "$ROOT/LICENSE" "$ROOT/CHANGELOG.md"; do
    [ -f "$required" ] || { echo "build-windows.sh: отсутствует $required" >&2; exit 1; }
done

BUILD_CMD="cargo build --release --locked --target $TARGET -p max_app --no-default-features"

echo "build-windows.sh: M.A.X. Rust client $VERSION → Windows x86_64 (headless)"
echo "  образ:     $RUST_IMAGE"
echo "  target:    $TARGET (mingw-w64)"
echo "  features:  <нет: --no-default-features, без sdl*, без sound>"
echo "  каталог:   $OUT_HOST/$BUNDLE_NAME"
echo "  архив:     $OUT_HOST/$BUNDLE_NAME.zip"
if [ "$DO_WINE" = "1" ]; then
    echo "  wine-smoke: да (--help + --demo SAVE1.CAM --turns 1)"
else
    echo "  wine-smoke: нет (--no-wine)"
fi

if [ "$DRY_RUN" = "1" ]; then
    echo
    echo "build-windows.sh: dry-run — команды, которые были бы выполнены:"
    printf '  mkdir -p %q\n' "$OUT_HOST/$BUNDLE_NAME"
    printf '  cp max-brh/../LICENSE CHANGELOG.md README.txt → %q\n' "$OUT_HOST/$BUNDLE_NAME"
    printf '  docker run --rm -v %q -v %q -v %q -w /src/max-brh %q bash -s\n' \
        "$ROOT:/src" "$OUT_HOST:/out" "$CARGO_HOME_DIR:/cargo-home" "$RUST_IMAGE"
    printf '    apt-get install gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 file zip%s\n' \
        "$([ "$DO_WINE" = "1" ] && printf ' wine wine64' || printf '')"
    printf '    rustup target add %s\n' "$TARGET"
    printf '    %s\n' "$BUILD_CMD"
    printf '    → /out/%s/max-brh.exe + SHA256SUMS + zip\n' "$BUNDLE_NAME"
    printf '    wine max-brh.exe --help; wine max-brh.exe --demo SAVE1.CAM --turns 1\n'
    exit 0
fi

command -v docker >/dev/null 2>&1 || { echo "build-windows.sh: docker не найден в PATH" >&2; exit 1; }
mkdir -p "$CARGO_HOME_DIR" "$(dirname "$SMOKE_LOG")"

# Каталог артефакта готовится на хосте: README.txt, лицензия и changelog не
# зависят от сборки, а контейнеру остаётся положить рядом exe и посчитать хеши.
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE"
cp "$ROOT/LICENSE" "$BUNDLE/LICENSE"
cp "$ROOT/CHANGELOG.md" "$BUNDLE/CHANGELOG.md"
cat > "$BUNDLE/README.txt" <<EOF
M.A.X. Rust client ${VERSION} — Windows x86_64 (headless)
========================================================

Собран кросс-компиляцией x86_64-pc-windows-gnu (mingw-w64) в Docker:
  ${BUILD_CMD}
Без фич sdl* и sound: окно SDL3 и звук в этот бинарь не входят (см. ниже).
Клиент не содержит данных игры: нужны MAX.RES, PATCHES.RES, COLOR.PAL,
*.WRL и *.SPW из оригинальной M.A.X. (собственность Interplay Productions).

Запуск (headless)
-----------------
  max-brh.exe --help
  max-brh.exe --demo SAVE1.CAM --turns 5          # прогон сейва → digest
  max-brh.exe --new-game --world 12 --seed 42 --out frame.ppm
  max-brh.exe --connect HOST:8082 --join GAME_ID  # Horizon gRPC + ENet, без окна

Запуск без флагов (GUI) в этой headless-сборке недоступен: клиент скажет,
что нужна сборка с feature sdl (она входит в default-features max_app), и
предложит headless-флаги выше. Обычная сборка из исходников
(\`cargo run -p max_app\` без --no-default-features) GUI открывает.

Данные игры указываются флагами:
  --data-dir <каталог>     MAX.RES и COLOR.PAL
  --assets-dir <каталог>   PATCHES.RES
  --world-dir <каталог>    *.WRL (если не рядом с MAX.RES)
  --max-res/--patches/--palette — точные пути

Зависимости
-----------
  Только системные DLL Windows: kernel32, ws2_32, winmm, msvcrt, bcryptprimitives
  (нет libgcc_s_seh-1.dll / libwinpthread-1.dll — mingw-рантайм не требуется).
  Horizon-эндпоинт — plaintext http://, TLS-стек в бинарь не входит.

Полная сборка с окном SDL3 и звуком — ТОЛЬКО на Windows-хосте
-------------------------------------------------------------
Кросс-сборка из Linux не тянет SDL3: sdl3-sys собирает SDL3 через CMake из
исходников (или vcpkg) и линкует Windows-импорты, а rodio/cpal на Windows
требует WASAPI-заголовков. Поэтому интерактивный клиент собирается на
Windows-машине с Rust 1.94.0 и MSVC:

  # 1. Rust + MSVC
  rustup toolchain install 1.94.0
  rustup default 1.94.0
  rustup target add x86_64-pc-windows-msvc
  # 2. Visual Studio Build Tools 2022: "Desktop development with C++"
  #    (MSVC, Windows SDK, CMake) — обязательны для линковки SDL3.
  # 3. SDL3 через vcpkg
  git clone https://github.com/microsoft/vcpkg C:\\vcpkg
  C:\\vcpkg\\bootstrap-vcpkg.bat
  C:\\vcpkg\\vcpkg install sdl3:x64-windows
  set VCPKG_ROOT=C:\\vcpkg          # PowerShell: \$env:VCPKG_ROOT = "C:\\vcpkg"
  # 4. Сборка (vcpkg-фича max_app/max_render). --no-default-features обязателен:
  #    иначе к sdl-vcpkg добавится default sdl-from-source (SDL3 через CMake).
  cd max-brh
  cargo build --release -p max_app --no-default-features --features sdl-vcpkg
  #    вариант без vcpkg: --features sdl-from-source (нужен CMake + MSVC) —
  #    он же default, поэтому достаточно \`cargo run -p max_app\` / \`cargo build -p max_app\`
  # 5. SDL3.dll рядом с exe (vcpkg: C:\\vcpkg\\installed\\x64-windows\\bin\\SDL3.dll)
  target\\release\\max-brh.exe

  Звук (feature sound) добавляется к фичам: --features sdl-vcpkg,sound;
  отдельные DLL не нужны (WASAPI — часть Windows).

Документация
------------
  README.txt (этот файл), CHANGELOG.md, LICENSE.
  Полный чек-лист релиза — max-brh-docs/development/release-checklist.md в репозитории (§6 —
  Windows x64: headless кросс-сборка и полная SDL-сборка на хосте).

Лицензия
--------
  Код клиента — MIT (см. LICENSE). M.A.X. (C) 1996 Interplay Productions;
  данные игры лицензией клиента не покрываются.
EOF

echo
echo "== сборка в $RUST_IMAGE (mingw-w64 + $TARGET) =="
# Скрипт стадии передаётся контейнеру через stdin (bash -s), поэтому кавычки
# хоста не участвуют: всё, что ниже, исполняется как есть.
docker run --rm -i \
    -v "$ROOT:/src" \
    -v "$OUT_HOST:/out" \
    -v "$CARGO_HOME_DIR:/cargo-home" \
    -e CARGO_HOME=/cargo-home \
    -e BUNDLE_NAME="$BUNDLE_NAME" \
    -e VERSION="$VERSION" \
    -e TARGET="$TARGET" \
    -e BUILD_CMD="$BUILD_CMD" \
    -e DO_WINE="$DO_WINE" \
    -w /src/max-brh \
    "$RUST_IMAGE" bash -s <<'CONTAINER'
set -uo pipefail
fail() { echo "СТАДИЯ: FAIL: $*" >&2; exit 1; }

export DEBIAN_FRONTEND=noninteractive
echo "СТАДИЯ: toolchain"

# --- 1. mingw-w64 (для vendored ENet 1.3.18), file и zip -------------------
if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    apt-get update -qq || fail "apt-get update"
    apt-get install -y -qq --no-install-recommends \
        gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 file zip \
        >/dev/null || fail "apt-get install mingw-w64/file/zip"
fi
for tool in x86_64-w64-mingw32-gcc x86_64-w64-mingw32-objdump file zip sha256sum; do
    command -v "$tool" >/dev/null 2>&1 || fail "нет инструмента $tool"
done
x86_64-w64-mingw32-gcc --version | head -1

# --- 2. target windows-gnu -------------------------------------------------
if ! rustup target list --installed | grep -qx "$TARGET"; then
    rustup target add "$TARGET" || fail "rustup target add $TARGET"
fi
rustc --version
cargo --version

# --- 3. wine (опционально) -------------------------------------------------
WINE_STATUS="skipped"
if [ "${DO_WINE:-1}" = "1" ]; then
    if ! command -v wine >/dev/null 2>&1; then
        apt-get update -qq >/dev/null 2>&1 || true
        apt-get install -y -qq --no-install-recommends wine wine64 >/dev/null 2>&1 || true
    fi
    if command -v wine >/dev/null 2>&1; then
        WINE_STATUS="present"
        echo "wine: $(wine --version 2>/dev/null || echo '?')"
    else
        WINE_STATUS="unavailable"
        echo "wine: не установился — wine-smoke будет пропущен" >&2
    fi
fi

# --- 4. сборка -------------------------------------------------------------
echo "СТАДИЯ: build"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
$BUILD_CMD || fail "$BUILD_CMD"
EXE="target/$TARGET/release/max-brh.exe"
[ -f "$EXE" ] || fail "после сборки нет $EXE"

# --- 5. проверки PE --------------------------------------------------------
echo "СТАДИЯ: verify"
file "$EXE"
EXE_SIZE="$(stat -c %s "$EXE")"
head -c 2 "$EXE" | grep -q '^MZ' || fail "нет MZ-заголовка"
# Headless-бинарь обязан остаться Console (PE Subsystem=3): подсистема windows
# включается только с feature sdl (main.rs), иначе --help/--demo не печатают в
# терминал/CI. `file` пишет "(console)" для Subsystem=3, "(GUI)" — для 2.
FILE_INFO="$(file -b "$EXE")"
case "$FILE_INFO" in
    *"PE32+ executable (console) x86-64"*) : ;;
    *) fail "headless-бинарь должен быть Console (Subsystem=3): $FILE_INFO" ;;
esac
# Вывод objdump забираем в переменную: `objdump | grep -q` при `pipefail`
# получает SIGPIPE (grep -q выходит по первому совпадению) и роняет проверку.
OBJDUMP_HEADER="$(x86_64-w64-mingw32-objdump -f "$EXE" 2>&1 || true)"
case "$OBJDUMP_HEADER" in
    *pei-x86-64*) : ;;
    *) fail "objdump: не pei-x86-64 (ожидался PE32+ x86-64): $OBJDUMP_HEADER" ;;
esac
DLLS="$(x86_64-w64-mingw32-objdump -p "$EXE" | awk '/DLL Name:/ {print $3}' | sort -u | tr '\n' ' ')"
echo "PE: size=$EXE_SIZE, format=pei-x86-64, subsystem=console, imports: ${DLLS:-<нет>}"
case "$DLLS" in
    *libgcc_s*|*libwinpthread*) fail "mingw-рантайм в импортах: $DLLS" ;;
esac
printf 'SHA256(exe): '; sha256sum "$EXE" | cut -d' ' -f1

# --- 6. упаковка (sha256 + zip) -------------------------------------------
echo "СТАДИЯ: package"
BUNDLE="/out/$BUNDLE_NAME"
mkdir -p "$BUNDLE"
cp "$EXE" "$BUNDLE/max-brh.exe"
(
    cd "$BUNDLE"
    sha256sum max-brh.exe README.txt LICENSE CHANGELOG.md > SHA256SUMS
)
(
    cd /out
    rm -f "$BUNDLE_NAME.zip" "$BUNDLE_NAME.zip.sha256"
    zip -X -r -q "$BUNDLE_NAME.zip" "$BUNDLE_NAME" || fail "zip"
    sha256sum "$BUNDLE_NAME.zip" > "$BUNDLE_NAME.zip.sha256"
)
echo "СТАДИЯ: packaged $(stat -c %s "/out/$BUNDLE_NAME.zip") bytes zip"

# --- 7. wine-smoke ---------------------------------------------------------
echo "СТАДИЯ: wine"
if [ "${DO_WINE:-1}" != "1" ] || [ "$WINE_STATUS" != "present" ]; then
    echo "WINE: skipped ($WINE_STATUS)"
else
    export WINEPREFIX=/tmp/wineprefix WINEDEBUG=-all
    WORK=/tmp/wine-smoke
    rm -rf "$WORK"; mkdir -p "$WORK"
    cp "$BUNDLE/max-brh.exe" "$WORK/"
    SAVE=/src/max-original/SAVE1.CAM
    if [ -f "$SAVE" ]; then
        cp "$SAVE" "$WORK/"
    else
        echo "wine: нет $SAVE — --demo будет пропущен" >&2
    fi
    cd "$WORK"
    LOG=/src/build/smoke-logs/windows-wine-smoke.log
    : > "$LOG"

    # std Rust импортирует bcryptprimitives!ProcessPrng, а в wine 8.0 (Debian
    # bookworm) этой DLL нет: exe падает на загрузке (exit 53, «Importing dlls
    # failed, status c0000135»). В более новых wine DLL есть, поэтому версию не
    # угадываем: сначала пробуем как есть, а если не вышло — подкладываем
    # ТЕСТОВЫЙ шим ProcessPrng → advapi32!SystemFunction036. В релизный zip он
    # не входит (на настоящей Windows bcryptprimitives.dll системная).
    build_shim() {
        cat > /tmp/shim.c <<'EOF'
#include <windows.h>
/* Stand-in for bcryptprimitives!ProcessPrng (wine 8 has no such DLL). */
typedef BOOLEAN (WINAPI *RtlGenRandomFn)(PVOID, ULONG);
__declspec(dllexport) BOOL WINAPI ProcessPrng(BYTE *pbData, SIZE_T cbData)
{
    static RtlGenRandomFn fn;
    if (!fn) {
        HMODULE adv = LoadLibraryA("advapi32.dll");
        if (!adv) return FALSE;
        fn = (RtlGenRandomFn)GetProcAddress(adv, "SystemFunction036");
        if (!fn) return FALSE;
    }
    return fn(pbData, (ULONG)cbData) ? TRUE : FALSE;
}
EOF
        x86_64-w64-mingw32-gcc -shared -o "$WORK/bcryptprimitives.dll" /tmp/shim.c
    }

    smoke() { # $1 — метка попытки ("как есть" / "с шимом bcryptprimitives")
        {
            echo
            echo "== попытка: $1 =="
            echo "== wine $(wine --version 2>/dev/null), хост $(uname -srm) =="
            echo "== max-brh.exe sha256: $(sha256sum max-brh.exe | cut -d' ' -f1) =="
            echo
            echo "== 1/2: max-brh.exe --help =="
            timeout 180 wine ./max-brh.exe --help 2>&1
            echo "EXIT_HELP=$?"
            if [ -f SAVE1.CAM ]; then
                echo
                echo "== 2/2: max-brh.exe --demo SAVE1.CAM --turns 1 =="
                timeout 300 wine ./max-brh.exe --demo SAVE1.CAM --turns 1 2>&1
                echo "EXIT_DEMO=$?"
            fi
        } | tee -a "$LOG"
    }

    SHIM=нет
    smoke "как есть"
    HELP_OK=0; DEMO_OK=0
    grep -q 'ИСПОЛЬЗОВАНИЕ' "$LOG" && grep -q 'EXIT_HELP=0' "$LOG" && HELP_OK=1
    grep -qE '^ticks=1 ' "$LOG" && grep -q 'EXIT_DEMO=0' "$LOG" && DEMO_OK=1
    if [ "$HELP_OK" != "1" ] || [ "$DEMO_OK" != "1" ]; then
        if build_shim 2>>"$LOG"; then
            SHIM=да
            echo "wine: exe не запустился — wine 8.0 без bcryptprimitives.dll, кладу тестовый шим ProcessPrng" >&2
            smoke "с тестовым шимом bcryptprimitives"
            HELP_OK=0; DEMO_OK=0
            grep -q 'ИСПОЛЬЗОВАНИЕ' "$LOG" && grep -q 'EXIT_HELP=0' "$LOG" && HELP_OK=1
            grep -qE '^ticks=1 ' "$LOG" && grep -q 'EXIT_DEMO=0' "$LOG" && DEMO_OK=1
        else
            echo "wine: шим bcryptprimitives не собрался" >&2
        fi
    fi

    if [ "$HELP_OK" = "1" ] && [ "$DEMO_OK" = "1" ]; then
        # Вердикт пишем и в консоль, и в лог: хост читает его из лога.
        echo "WINE: ok (--help + --demo 1 тик; шим bcryptprimitives: $SHIM)" | tee -a "$LOG"
    else
        echo "WINE: failed (help_ok=$HELP_OK demo_ok=$DEMO_OK, лог: build/smoke-logs/windows-wine-smoke.log)" \
            | tee -a "$LOG" >&2
    fi
fi

echo "СТАДИЯ: done"
CONTAINER

echo
echo "== проверка артефактов на хосте =="
[ -f "$BUNDLE/max-brh.exe" ] || { echo "build-windows.sh: нет $BUNDLE/max-brh.exe" >&2; exit 1; }
[ -f "$BUNDLE/SHA256SUMS" ] || { echo "build-windows.sh: нет $BUNDLE/SHA256SUMS" >&2; exit 1; }
[ -f "$ZIP" ] || { echo "build-windows.sh: нет $ZIP" >&2; exit 1; }
[ -f "$ZIP.sha256" ] || { echo "build-windows.sh: нет $ZIP.sha256" >&2; exit 1; }

EXE_HASH="$(cd "$BUNDLE" && sha256sum max-brh.exe | cut -d' ' -f1)"
EXE_SIZE="$(stat -c %s "$BUNDLE/max-brh.exe")"

echo
echo "Готово:"
echo "  $OUT_HOST/$BUNDLE_NAME/"
echo "    max-brh.exe   ${EXE_SIZE} байт"
echo "    SHA256        $EXE_HASH"
echo "  $OUT_HOST/$BUNDLE_NAME.zip"
sed 's/^/    /' "$ZIP.sha256" 2>/dev/null || true

if [ "$DO_WINE" = "1" ]; then
    if grep -q 'WINE: ok' "$SMOKE_LOG" 2>/dev/null; then
        echo "  wine-smoke:  OK (build/smoke-logs/windows-wine-smoke.log)"
    else
        echo "  wine-smoke:  не подтверждён — см. build/smoke-logs/windows-wine-smoke.log" >&2
        if [ "${WINE_STRICT:-0}" = "1" ]; then
            echo "build-windows.sh: WINE_STRICT=1 — wine-smoke не прошёл" >&2
            exit 1
        fi
    fi
fi
