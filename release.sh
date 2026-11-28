#!/usr/bin/env bash
# Build of the release Linux binary of the M.A.X. Rust client (M6.4).
#
#   bash release.sh --dry-run          # show the plan, build nothing
#   bash release.sh                    # build + package bin/release/
#   bash release.sh --tag              # the same + annotated tag v<version>
#   bash release.sh --features sdl-vcpkg,sound   # a custom feature set (by
#                                      # default sdl-from-source,sound;
#                                      # explicit features are combined with
#                                      # --no-default-features)
#   bash release.sh --out /tmp/rel     # a different artifact directory
#
# Requirements: Docker with Linux containers; network for apt-get (cmake,
# pkg-config, libasound2-dev) and crates.io. The build runs in
# rust:1.94.0-bookworm, no host toolchain needed.
#
# Artifacts:
#   bin/bin/release/max_app-<version>-linux-x86_64/        directory (binary + run.sh +
#                                                  README.txt + LICENSE +
#                                                  CHANGELOG.md + lib/ + SHA256SUMS)
#   bin/release/max_app-<version>-linux-x86_64.tar.gz  archive
#   bin/release/max_app-<version>-linux-x86_64.tar.gz.sha256
#
# The tag is created only with --tag and only on a clean working tree; pushing the tag is
# a separate deliberate action (see max-brh-docs/development/release-checklist.md §8).
set -euo pipefail

# Git Bash (MSYS2) rewrites POSIX paths in arguments (`-w /src/max-brh` →
# `C:/Program Files/Git/src/max-brh`) — we disable the conversion.
# This script does not build the Windows binary: headless is native on MSVC
# (max-brh-cicd/scripts/build-windows-console.ps1), UI — build-windows-ui.ps1.
if [ -n "${MSYSTEM:-}" ] || command -v cygpath >/dev/null 2>&1; then
    export MSYS_NO_PATHCONV=1
    export MSYS2_ARG_CONV_EXCL='*'
fi

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# Docker Desktop for Windows expects D:/..., not /d/...
if command -v cygpath >/dev/null 2>&1; then
    ROOT="$(cygpath -m "$ROOT")"
fi

RUST_IMAGE="${RUST_IMAGE:-rust:1.94.0-bookworm}"
FEATURES="${FEATURES:-sdl-from-source,sound}"
OUT="${OUT:-$ROOT/bin/release}"
DRY_RUN=0
MAKE_TAG=0

usage() {
    cat <<'EOF'
Использование: bash release.sh [--dry-run] [--tag] [--features LIST] [--out DIR]

  --dry-run        показать план сборки/упаковки и выйти (Docker не запускается)
  --tag            после успешной упаковки создать git-тег v<version>
                   (только на чистом рабочем дереве; тег не пушится)
  --features LIST  features max_app (по умолчанию sdl-from-source,sound;
                   пустая строка — консольная сборка клиента: --features '')
  --out DIR        каталог артефактов (по умолчанию <repo>/bin/release)
  -h, --help       эта справка

Переменные: RUST_IMAGE (rust:1.94.0-bookworm), FEATURES, OUT.
EOF
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --tag)
            MAKE_TAG=1
            shift
            ;;
        --features)
            [ "$#" -ge 2 ] || {
                echo "release.sh: --features требует список" >&2
                exit 2
            }
            FEATURES="$2"
            shift 2
            ;;
        --out)
            [ "$#" -ge 2 ] || {
                echo "release.sh: --out требует каталог" >&2
                exit 2
            }
            OUT="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "release.sh: неизвестный аргумент: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

mkdir -p "$OUT"
OUT="$(cd -- "$OUT" && pwd)"
if command -v cygpath >/dev/null 2>&1; then
    OUT_HOST="$(cygpath -m "$OUT")"
else
    OUT_HOST="$OUT"
fi

MANIFEST="$ROOT/max-brh/Cargo.toml"
[ -f "$MANIFEST" ] || {
    echo "release.sh: нет $MANIFEST" >&2
    exit 1
}

VERSION="$(awk -F'"' '/^version = / { print $2; exit }' "$MANIFEST")"
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'; then
    echo "release.sh: не удалось прочитать версию из $MANIFEST (получено '$VERSION')" >&2
    exit 1
fi

BUNDLE_NAME="max_app-${VERSION}-linux-x86_64"
BUNDLE="$OUT/$BUNDLE_NAME"
TARBALL="$OUT/$BUNDLE_NAME.tar.gz"
TARBALL_HASH="$TARBALL.sha256"
BINARY="$ROOT/max-brh/target/release/max-brh"
CARGO_HOME_DIR="$ROOT/build/cargo-home"

for required in "$ROOT/LICENSE" "$ROOT/CHANGELOG.md" "$ROOT/max-brh-docs/development/release-checklist.md"; do
    [ -f "$required" ] || {
        echo "release.sh: отсутствует обязательный файл $required" >&2
        exit 1
    }
done

# default max_app = ["sdl-from-source"], so we set the feature set explicitly and
# disable the default features: otherwise `--features sdl-vcpkg` would also add
# sdl-from-source, and the headless release (`--features ''`) would unexpectedly get a window.
if [ -n "$FEATURES" ]; then
    BUILD_CMD="cargo build --release -p max_app --no-default-features --features $FEATURES"
else
    BUILD_CMD="cargo build --release -p max_app --no-default-features"
fi
INNER="apt-get update -qq && apt-get install -y -qq gcc g++ cmake pkg-config libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev libxi-dev libxrandr-dev libxinerama-dev libxss-dev libxtst-dev libxfixes-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev wayland-protocols libdecor-0-dev libegl-dev libgles-dev libgl-dev libgbm-dev libdrm-dev libdbus-1-dev libudev-dev >/dev/null && $BUILD_CMD"

DOCKER_RUN=(docker run --rm
    -v "$ROOT:/src"
    -v "$CARGO_HOME_DIR:/cargo-home"
    -e "CARGO_HOME=/cargo-home"
    -w /src/max-brh
    "$RUST_IMAGE"
    bash -c "$INNER")

echo "release.sh: M.A.X. Rust client $VERSION"
echo "  образ:   $RUST_IMAGE"
echo "  features: ${FEATURES:-<нет, headless>}"
echo "  бинарь:  $BINARY"
echo "  каталог: $OUT_HOST/$BUNDLE_NAME"
echo "  архив:   $OUT_HOST/$BUNDLE_NAME.tar.gz"

if [ "$DRY_RUN" = "1" ]; then
    echo
    echo "release.sh: dry-run — команды, которые были бы выполнены:"
    printf '  mkdir -p %q\n' "$OUT_HOST/$BUNDLE_NAME/lib"
    printf '  docker run --rm -v %q -v %q -e CARGO_HOME=/cargo-home -w /src/max-brh %q bash -c %q\n' \
        "$ROOT:/src" "$CARGO_HOME_DIR:/cargo-home" "$RUST_IMAGE" "$INNER"
    printf '  cp %q %q\n' "$BINARY" "$BUNDLE/max-brh"
    printf '  cp -a max-brh/target/release/libSDL3.so* %q\n' "$BUNDLE/lib/"
    printf '  tar -C %q -czf %q %q\n' "$OUT_HOST" "$OUT_HOST/$BUNDLE_NAME.tar.gz" "$BUNDLE_NAME"
    if [ "$MAKE_TAG" = "1" ]; then
        echo "  git tag -a v$VERSION -m \"M.A.X. Rust client $VERSION\"  # только на чистом дереве"
    else
        echo "  (тег не создаётся; после проверки: bash release.sh --tag)"
    fi
    exit 0
fi

command -v docker >/dev/null 2>&1 || {
    echo "release.sh: docker не найден в PATH" >&2
    exit 1
}
command -v tar >/dev/null 2>&1 || {
    echo "release.sh: tar не найден в PATH" >&2
    exit 1
}
command -v sha256sum >/dev/null 2>&1 || {
    echo "release.sh: sha256sum не найден в PATH" >&2
    exit 1
}

mkdir -p "$CARGO_HOME_DIR"
echo
echo "== 1/3: cargo build --release ($FEATURES) в $RUST_IMAGE =="
"${DOCKER_RUN[@]}"

[ -f "$BINARY" ] || {
    echo "release.sh: после сборки нет $BINARY" >&2
    exit 1
}

echo
echo "== 2/3: упаковка $BUNDLE_NAME =="
rm -rf "$BUNDLE"
mkdir -p "$BUNDLE/lib"
cp "$BINARY" "$BUNDLE/max-brh"
chmod +x "$BUNDLE/max-brh"
cp "$ROOT/LICENSE" "$BUNDLE/LICENSE"
cp "$ROOT/CHANGELOG.md" "$BUNDLE/CHANGELOG.md"

# SDL3 from sdl-from-source is a dynamic library; we place it next to the binary
# so that the artifact does not depend on the system SDL3. If the build is static,
# the lib directory stays empty (this is fine).
shopt -s nullglob
SDL_LIBS=("$ROOT"/max-brh/target/release/libSDL3.so*)
shopt -u nullglob
if [ "${#SDL_LIBS[@]}" -gt 0 ]; then
    cp -a "${SDL_LIBS[@]}" "$BUNDLE/lib/"
fi

cat > "$BUNDLE/run.sh" <<'EOF'
#!/usr/bin/env bash
# Launch M.A.X. Rust client from the unpacked archive.
set -euo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
export LD_LIBRARY_PATH="$HERE/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$HERE/max-brh" "$@"
EOF
chmod +x "$BUNDLE/run.sh"

cat > "$BUNDLE/README.txt" <<EOF
M.A.X. Rust client ${VERSION} — Linux x86_64
==========================================

Собран из тега v${VERSION} (features: ${FEATURES:-headless}).
Клиент не содержит данных игры: нужны MAX.RES, PATCHES.RES, COLOR.PAL,
*.WRL и *.SPW из оригинальной M.A.X. (собственность Interplay Productions).

Запуск
------
  ./run.sh --help
  ./run.sh                         # GUI: меню + партия (нужен дисплей)
  ./run.sh --ui                    # то же (совместимый alias)
  ./run.sh --connect HOST:8082 --join GAME_ID --sdl
  ./run.sh --demo SAVE1.CAM --turns 5

Данные игры указываются флагами:
  --data-dir <каталог>     MAX.RES и COLOR.PAL
  --assets-dir <каталог>   PATCHES.RES
  --world-dir <каталог>    *.WRL (если не рядом с MAX.RES)
  --max-res/--patches/--palette — точные пути

Зависимости
-----------
  libasound2 (ALSA) — feature sound; libSDL3.so.0 уже лежит в lib/.
  X11/Wayland — для окна; --demo/--new-game/--connect без --sdl работают
  без дисплея.

Документация
------------
  README.txt (этот файл), CHANGELOG.md, LICENSE.
  Полный чек-лист релиза — max-brh-docs/development/release-checklist.md в репозитории.

Лицензия
--------
  Код клиента — MIT (см. LICENSE). M.A.X. (C) 1996 Interplay Productions;
  данные игры лицензией клиента не покрываются.
EOF

echo "== 3/3: sha256 + архив =="
FILES=(max-brh run.sh README.txt CHANGELOG.md LICENSE)
shopt -s nullglob
for lib in "$BUNDLE"/lib/*; do
    FILES+=("lib/$(basename "$lib")")
done
shopt -u nullglob
(
    cd "$BUNDLE"
    sha256sum "${FILES[@]}" > SHA256SUMS
)
rm -f "$TARBALL" "$TARBALL_HASH"
tar -C "$OUT" -czf "$TARBALL" "$BUNDLE_NAME"
(
    cd "$OUT"
    sha256sum "$BUNDLE_NAME.tar.gz" > "$(basename "$TARBALL_HASH")"
)

echo
echo "Готово:"
echo "  $OUT_HOST/$BUNDLE_NAME"
echo "  $OUT_HOST/$BUNDLE_NAME.tar.gz"
sed 's/^/  /' "$TARBALL_HASH"

if [ "$MAKE_TAG" = "1" ]; then
    echo
    if git -C "$ROOT" rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null; then
        echo "release.sh: тег v$VERSION уже существует — не перезаписываю" >&2
        exit 1
    fi
    if [ -n "$(git -C "$ROOT" status --porcelain)" ]; then
        echo "release.sh: рабочее дерево грязное — тег не создаю." >&2
        echo "  Закоммитьте релиз и повторите: bash release.sh --tag" >&2
        exit 1
    fi
    git -C "$ROOT" tag -a "v$VERSION" -m "M.A.X. Rust client $VERSION"
    echo "Создан тег v$VERSION (не отправлен): git push origin v$VERSION"
else
    echo
    echo "Тег не создан. После проверки артефактов:"
    echo "  bash release.sh --tag          # чистое дерево + сборка + тег"
    echo "  git push origin v$VERSION"
fi
