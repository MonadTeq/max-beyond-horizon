#!/usr/bin/env bash
# Builds and packages the portable Linux distribution (stage 4 of
# max-brh-docs/development/distro-installers.md, §D/§F).
#
# Runs INSIDE max-brh-installer/linux/Dockerfile (Ubuntu 22.04 + Rust 1.94.0):
#
#   docker run --rm -v <repo>:/src -v <work>:/work max-brh-linux-dist:0.1.28a \
#       bash /src/max-brh-installer/linux/build-distro.sh
#
# Inputs (environment):
#   FEATURES      max_app features (default: sdl-from-source,sound)
#   VERSION       shown product version (default: max_app::version::DISPLAY_VERSION)
#   SKIP_BUILD=1  package an existing $CARGO_TARGET_DIR/release/max-brh
#   JOBS          cargo -j (default: nproc)
#   KEEP_TAR=1    keep the uncompressed .tar next to the archive
#
# Outputs under /work/out (bind-mounted, outside git):
#   max-brh-<version>-linux/                      the portable catalog
#   max-brh-<version>-linux.tar.gz (or .tar.xz)   archive, the smaller of the two
#   MAX-BRH-<version>-x86_64.AppImage             single-file AppImage
#   SHA256SUMS                                    artifact hashes
#   staging-report.json                           what was packaged and how it was verified
#
# The data files are copied from the read-only oracles (max-original/,
# max-cpp-port/assets/) and every file is checked against data-inventory.json
# (size + sha256) — the package data stays byte-for-byte the oracle's.
set -euo pipefail

SRC="${SRC:-/src}"
WORK="${WORK:-/work}"
OUT="$WORK/out"
APPDIR="$WORK/AppDir"
FEATURES="${FEATURES:-sdl-from-source,sound}"
JOBS="${JOBS:-$(nproc)}"
SKIP_BUILD="${SKIP_BUILD:-0}"
KEEP_TAR="${KEEP_TAR:-0}"

# Deterministic timestamp of every packaged file (stage 1/3 use the same value).
STAMP="2026-11-28 00:00:00 UTC"

say() { printf '\n== %s ==\n' "$*"; }
die() { printf 'build-distro: %s\n' "$*" >&2; exit 1; }

[ -f "$SRC/max-brh/Cargo.toml" ] || die "source tree not mounted at $SRC (docker run -v <repo>:/src)"
[ -f "$SRC/max-brh-installer/data-inventory.json" ] || die "missing data-inventory.json"
[ -d "$SRC/max-brh-vendor/SDL" ] || die "missing max-brh-vendor/SDL (sdl-from-source submodule)"
[ -d "$SRC/target/remaster" ] || die "missing target/remaster (the HD remaster is mandatory)"
mkdir -p "$WORK" "$OUT"
git config --global --add safe.directory "$SRC" 2>/dev/null || true

# --- 1. Version -------------------------------------------------------------------
if [ -z "${VERSION:-}" ]; then
    VERSION="$(sed -n 's/^pub const DISPLAY_VERSION: &str = "\(.*\)";$/\1/p' \
        "$SRC/max-brh/crates/max_app/src/version.rs" | head -n 1)"
fi
printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+[a-z]?$' \
    || die "cannot determine the product version (got '$VERSION')"

NAME="max-brh-${VERSION}-linux"
STAGE="$OUT/$NAME"
RELEASE_NAME="MAX-BRH-${VERSION}-x86_64"
APPIMAGE="$OUT/$RELEASE_NAME.AppImage"
TARGET_DIR="$WORK/target/linux"
BIN="$TARGET_DIR/release/max-brh"
export CARGO_TARGET_DIR="$TARGET_DIR"
export CARGO_HOME="${CARGO_HOME:-/work/cargo-home}"
export CARGO_NET_RETRY=5
export CARGO_HTTP_TIMEOUT=120
mkdir -p "$CARGO_HOME"

say "MAX-BRH $VERSION — Linux x86_64 portable distribution"
echo "  features: $FEATURES"
echo "  staging:  $STAGE"
echo "  out:      $OUT"

# --- 2. Build the client + SDL3 ----------------------------------------------------
if [ "$SKIP_BUILD" = "1" ]; then
    say "1/7 build: skipped (SKIP_BUILD=1)"
    [ -x "$BIN" ] || die "SKIP_BUILD=1 but $BIN does not exist"
else
    say "1/7 build: cargo build --release ($FEATURES)"
    (
        cd "$SRC/max-brh"
        cargo build --release -p max_app --no-default-features --features "$FEATURES" -j "$JOBS"
    )
    [ -x "$BIN" ] || die "no binary after the build: $BIN"
fi

RELEASE_DIR="$TARGET_DIR/release"
SDL_LIBS=()
while IFS= read -r lib; do SDL_LIBS+=("$lib"); done < <(find "$RELEASE_DIR" -maxdepth 1 -name 'libSDL3.so*' | sort)
[ "${#SDL_LIBS[@]}" -gt 0 ] || die "no libSDL3.so* in $RELEASE_DIR (sdl-from-source expected)"

file "$BIN" | grep -q 'ELF 64-bit' || die "the built binary is not a Linux x86_64 ELF: $(file "$BIN")"

# ldd before packaging: the shared SDL3 must resolve from the build tree.
NOT_FOUND="$(LD_LIBRARY_PATH="$RELEASE_DIR" ldd "$BIN" | grep -c 'not found' || true)"
[ "$NOT_FOUND" = "0" ] || {
    LD_LIBRARY_PATH="$RELEASE_DIR" ldd "$BIN" | grep 'not found' >&2
    die "$NOT_FOUND unresolved libraries in the build tree"
}
echo "  binary: $(stat -c %s "$BIN") bytes, $(basename "$(readlink -f "${SDL_LIBS[0]}")")"

# --- 3. Stage the portable catalog -------------------------------------------------
say "2/7 stage the catalog: $NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE/lib" "$STAGE/data" "$STAGE/remaster" "$STAGE/saves" "$STAGE/missions"

install -m 755 "$BIN" "$STAGE/max-brh"
for lib in "${SDL_LIBS[@]}"; do
    cp -a "$lib" "$STAGE/lib/"
done
# The package must run from any location: resolve libSDL3.so.0 by rpath, not by
# the caller's LD_LIBRARY_PATH (run.sh keeps the explicit path as well).
patchelf --set-rpath '$ORIGIN/lib' "$STAGE/max-brh"
NOT_FOUND="$(ldd "$STAGE/max-brh" | grep -c 'not found' || true)"
[ "$NOT_FOUND" = "0" ] || {
    ldd "$STAGE/max-brh" | grep 'not found' >&2
    die "$NOT_FOUND unresolved libraries after staging"
}

# --- 4. Data: 509 files, byte-for-byte the inventory -------------------------------
say "3/7 data: 509 verified files from data-inventory.json"
INVENTORY="$SRC/max-brh-installer/data-inventory.json"
EXPECTED_FILES="$(jq -r '.totals.files' "$INVENTORY")"
EXPECTED_BYTES="$(jq -r '.totals.bytes' "$INVENTORY")"
LIST="$(mktemp)"
jq -r '.files[] | [.path, (.size|tostring), .sha256] | @tsv' "$INVENTORY" > "$LIST"
ACTUAL_FILES="$(wc -l < "$LIST")"
[ "$ACTUAL_FILES" = "$EXPECTED_FILES" ] || die "inventory lists $ACTUAL_FILES files, totals say $EXPECTED_FILES"

# The oracles/asset directories are case-sensitive on Linux while the inventory
# was produced on Windows: index every source file by its lower-case name. The
# inventory itself holds flat file names; max-original/ wins over the C++ asset
# directory when both carry the same name (the stage.ps1 root order).
declare -A SOURCE_BY_LOWER=()
while IFS= read -r file; do
    base="${file##*/}"
    [ -n "${SOURCE_BY_LOWER[${base,,}]:-}" ] || SOURCE_BY_LOWER["${base,,}"]="$file"
done < <(find "$SRC/max-original" "$SRC/max-cpp-port/assets" -type f 2>/dev/null)

COPIED_FILES=0
COPIED_BYTES=0
while IFS=$'\t' read -r rel size sha; do
    src="${SOURCE_BY_LOWER[${rel,,}]:-}"
    [ -n "$src" ] || die "game data file not found in the oracles: $rel"

    actual_size="$(stat -c %s "$src")"
    [ "$actual_size" = "$size" ] || die "size mismatch for $rel: $actual_size != $size"
    actual_sha="$(sha256sum "$src" | cut -d' ' -f1)"
    [ "$actual_sha" = "$sha" ] || die "sha256 mismatch for $rel: $actual_sha != $sha"

    dst="$STAGE/data/$rel"
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
    chmod 644 "$dst"
    touch -d "$STAMP" "$dst"
    COPIED_FILES=$((COPIED_FILES + 1))
    COPIED_BYTES=$((COPIED_BYTES + actual_size))
done < "$LIST"
rm -f "$LIST"

[ "$COPIED_FILES" = "$EXPECTED_FILES" ] || die "staged $COPIED_FILES data files, expected $EXPECTED_FILES"
[ "$COPIED_BYTES" = "$EXPECTED_BYTES" ] || die "staged $COPIED_BYTES data bytes, expected $EXPECTED_BYTES"
echo "  data: $COPIED_FILES files, $COPIED_BYTES bytes, sha256 verified"

# --- 5. Mandatory remaster ---------------------------------------------------------
say "4/7 remaster (mandatory): target/remaster"
REMASTER_SRC="$SRC/target/remaster"
[ -f "$REMASTER_SRC/remaster-manifest.json" ] || die "missing $REMASTER_SRC/remaster-manifest.json"
REMASTER_KTX2="$(find "$REMASTER_SRC" -maxdepth 1 -name '*.ktx2' | wc -l)"
[ "$REMASTER_KTX2" -gt 0 ] || die "no .ktx2 layers in $REMASTER_SRC"
cp -a "$REMASTER_SRC/." "$STAGE/remaster/"
find "$STAGE/remaster" -type f -exec touch -d "$STAMP" {} +
REMASTER_FILES="$(find "$STAGE/remaster" -type f | wc -l)"
REMASTER_BYTES="$(find "$STAGE/remaster" -type f -printf '%s\n' | awk '{ s += $1 } END { print s + 0 }')"
echo "  remaster: $REMASTER_FILES files, $REMASTER_BYTES bytes"

# --- 6. Player-facing files, licenses, launcher ------------------------------------
say "5/7 documentation, licenses, launcher"
cp "$SRC/LICENSE" "$STAGE/LICENSE"
cp "$SRC/CHANGELOG.md" "$STAGE/CHANGELOG.md"
cp "$SRC/settings.jsonc" "$STAGE/settings.jsonc"

SDL_LICENSE="$SRC/max-brh-vendor/SDL/LICENSE.txt"
ENET_LICENSE="$SRC/max-brh/crates/max_net/vendor/enet/LICENSE"
[ -f "$SDL_LICENSE" ] || die "missing $SDL_LICENSE"
[ -f "$ENET_LICENSE" ] || die "missing $ENET_LICENSE"
{
    cat <<EOF
M.A.X.: Beyond Rust Horizon $VERSION - third-party licenses
===============================================================================

This package contains the following third-party software. The full license
texts are reproduced below; the client's own code is MIT (see LICENSE).

1. SDL3 (Simple DirectMedia Layer 3) - zlib license
   Copyright (C) 1997-2025 Sam Lantinga and SDL contributors.
   Project: https://www.libsdl.org/

2. ENet 1.3.18 - MIT license
   Copyright (C) 2002-2024 Lee Salzman and ENet contributors.
   Project: https://github.com/lsalzman/enet

3. Rust crates used by the client (MIT and/or Apache-2.0), including anyhow,
   serde, serde_json, tokio, tracing, prost, pentagram, egui and rodio/cpal
   (sound). The complete license texts are part of the source archive and the
   crates' published packages.

4. Game data
   M.A.X.: Mechanized Assault & Exploration is a trademark of Interplay
   Entertainment Corp. The original game assets (c) Interplay 1996. This
   project is a free, non-commercial fan remake, is not affiliated with or
   endorsed by the rights holder, and redistributes the assets without
   commercial intent.

--------------------------------------------------------------------------------
SDL3 LICENSE
--------------------------------------------------------------------------------
$(cat "$SDL_LICENSE")

--------------------------------------------------------------------------------
ENet LICENSE
--------------------------------------------------------------------------------
$(cat "$ENET_LICENSE")
EOF
} > "$STAGE/THIRD_PARTY_LICENSES.txt"

cat > "$STAGE/run.sh" <<'EOF'
#!/usr/bin/env bash
# Launches M.A.X. from the unpacked tar.gz catalog.
set -euo pipefail
HERE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
export LD_LIBRARY_PATH="$HERE/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
exec "$HERE/max-brh" "$@"
EOF
chmod 755 "$STAGE/run.sh"

cat > "$STAGE/README.txt" <<EOF
M.A.X.: Beyond Rust Horizon ${VERSION} — Linux x86_64, переносимая сборка
========================================================================

Один каталог игры: клиент, данные, HD-ремастер и сейвы лежат рядом, системная
установка не нужна (ни /usr, ни \$XDG, ни домашний каталог не используются).

Запуск
------
  ./run.sh                                  # или сразу ./max-brh
  ./max-brh --help                          # режимы и флаги
  ./max-brh                                 # GUI: меню + партия (нужен дисплей)
  ./max-brh --demo saves/SAVE1.CAM --turns 5   # без дисплея

Сейвы, миссии и настройки — ВНУТРИ каталога игры
------------------------------------------------
  saves/            сейвы игрока (C++ FilePathGamePref)
  missions/         сейвы-миссии (C++ FilePathGameData)
  preferences.ini   настройки игрока
  settings.jsonc    разрешение / сглаживание / масштаб окна
Каталог можно перенести целиком — прогресс сохранится.

Если вы скачали .AppImage
-------------------------
Запускайте файл двойным кликом; saves/, missions/ и settings.jsonc появятся
рядом с .AppImage (образ только для чтения, данные игрока — снаружи).

Состав
------
  max-brh                   клиент (features: ${FEATURES})
  lib/libSDL3.so*           SDL3 в комплекте (rpath \$ORIGIN/lib)
  data/                     ресурсы M.A.X.: ${COPIED_FILES} файлов, оригинальные байты
  remaster/                 HD-ремастер, обязательная часть
  SHA256SUMS                контрольные суммы всех файлов пакета

Зависимости
-----------
  glibc >= 2.35 (Ubuntu 22.04 и новее), libasound2 (звук), X11 или Wayland
  для окна. SDL3 и данные игры входят в пакет.

Документация и лицензии
-----------------------
  README.txt (этот файл), CHANGELOG.md, LICENSE, THIRD_PARTY_LICENSES.txt.
  Код клиента — MIT. M.A.X. (C) 1996 Interplay; данные игры лицензией клиента
  не покрываются. Пакет — бесплатный некоммерческий фан-ремейк.
EOF

# Every packaged file gets the same timestamp and a normalized mode: the Windows
# bind mount marks every source file executable, and a package must not ship an
# executable README.txt. Only the launcher scripts stay executable.
find "$STAGE" -type d -exec chmod 755 {} +
find "$STAGE" -type f -exec chmod 644 {} +
chmod 755 "$STAGE/max-brh" "$STAGE/run.sh"
find "$STAGE" -exec touch -h -d "$STAMP" {} +

say "6/7 archive + AppImage"
(
    cd "$STAGE"
    find . -type f ! -name SHA256SUMS -printf '%P\n' | LC_ALL=C sort | while IFS= read -r file; do
        sha256sum -- "$file"
    done > SHA256SUMS
)
echo "  SHA256SUMS: $(wc -l < "$STAGE/SHA256SUMS") files"

# --- 7. Archives: tar.gz vs tar.xz, keep the smaller ------------------------------
TAR="$WORK/$NAME.tar"
tar -C "$OUT" \
    --sort=name --mtime="$STAMP" --owner=0 --group=0 --numeric-owner \
    --format=gnu -cf "$TAR" "$NAME"

GZ="$OUT/$NAME.tar.gz"
XZ="$OUT/$NAME.tar.xz"
rm -f "$GZ" "$XZ"
gzip -9nc "$TAR" > "$GZ"
GZ_SIZE="$(stat -c %s "$GZ")"
echo "  tar.gz: $GZ_SIZE bytes"
if xz -T0 -6 -c "$TAR" > "$XZ" 2>/dev/null; then
    XZ_SIZE="$(stat -c %s "$XZ")"
    echo "  tar.xz: $XZ_SIZE bytes"
else
    XZ_SIZE=0
    rm -f "$XZ"
fi
if [ "$XZ_SIZE" -gt 0 ] && [ "$XZ_SIZE" -lt "$GZ_SIZE" ]; then
    ARCHIVE="$XZ"
    rm -f "$GZ"
    echo "  archive: tar.xz is $(( (GZ_SIZE - XZ_SIZE) / 1024 / 1024 )) MiB smaller"
else
    ARCHIVE="$GZ"
    [ "$XZ_SIZE" -gt 0 ] && rm -f "$XZ"
    echo "  archive: tar.gz"
fi
[ "$KEEP_TAR" = "1" ] && echo "  uncompressed tar kept: $TAR" || rm -f "$TAR"

# --- 8. AppDir + AppImage ---------------------------------------------------------
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib"
install -m 755 "$STAGE/max-brh" "$APPDIR/usr/bin/max-brh"
# Inside the AppImage the same SDL3 lives one directory up: usr/bin -> usr/lib.
patchelf --set-rpath '$ORIGIN/../lib' "$APPDIR/usr/bin/max-brh"
cp -a "$STAGE/lib/." "$APPDIR/usr/lib/"
cp -a "$STAGE/data" "$APPDIR/data"
cp -a "$STAGE/remaster" "$APPDIR/remaster"
cp "$STAGE/LICENSE" "$STAGE/THIRD_PARTY_LICENSES.txt" "$STAGE/README.txt" \
    "$STAGE/CHANGELOG.md" "$STAGE/settings.jsonc" "$APPDIR/"
install -m 755 "$SRC/max-brh-installer/linux/AppRun" "$APPDIR/AppRun"
install -m 644 "$SRC/max-brh-installer/linux/max-brh.desktop" "$APPDIR/max-brh.desktop"
convert "$SRC/max-brh-assets/game-icon.png" -resize 512x512 -strip "$APPDIR/max-brh.png"
( cd "$APPDIR" && ln -sf max-brh.png .DirIcon )
find "$APPDIR" -type d -exec chmod 755 {} +
find "$APPDIR" -type f -exec chmod 644 {} +
chmod 755 "$APPDIR/AppRun" "$APPDIR/usr/bin/max-brh"
find "$APPDIR" -exec touch -h -d "$STAMP" {} +

NOT_FOUND="$(ldd "$APPDIR/usr/bin/max-brh" | grep -c 'not found' || true)"
[ "$NOT_FOUND" = "0" ] || {
    ldd "$APPDIR/usr/bin/max-brh" | grep 'not found' >&2
    die "$NOT_FOUND unresolved libraries in the AppDir"
}

rm -f "$APPIMAGE"
say "appimagetool $RELEASE_NAME.AppImage"
ARCH=x86_64 appimagetool --no-appstream "$APPDIR" "$APPIMAGE" >/dev/null
[ -s "$APPIMAGE" ] || die "appimagetool produced no AppImage"
chmod 755 "$APPIMAGE"
head -c 4 "$APPIMAGE" | od -An -tx1 | tr -d ' \n' | grep -q '7f454c46' \
    || die "the AppImage has no ELF header"
echo "  AppImage: $(stat -c %s "$APPIMAGE") bytes"

# --- 9. Artifact hashes + report --------------------------------------------------
rm -f "$OUT/SHA256SUMS"
(
    cd "$OUT"
    sha256sum "$(basename "$APPIMAGE")" "$(basename "$ARCHIVE")" > SHA256SUMS
)

DATA_CLASSES="$(jq -c '[.classes[] | {class: .class, files: .files, bytes: .bytes}]' "$INVENTORY")"
BINARY_SHA="$(sha256sum "$STAGE/max-brh" | cut -d' ' -f1)"
ARCHIVE_SHA="$(sha256sum "$ARCHIVE" | cut -d' ' -f1)"
APPIMAGE_SHA="$(sha256sum "$APPIMAGE" | cut -d' ' -f1)"
APPIMAGE_BYTES="$(stat -c %s "$APPIMAGE")"
ARCHIVE_BYTES="$(stat -c %s "$ARCHIVE")"
APPDIR_FILES="$(find "$APPDIR" -type f | wc -l)"
GIT_HEAD="$(git -C "$SRC" rev-parse HEAD 2>/dev/null || echo unknown)"
ORACLE_STATUS="$(git -C "$SRC" status --porcelain -- max-original max-cpp-port max-original-dasm 2>/dev/null || true)"
[ -z "$ORACLE_STATUS" ] && ORACLE_CLEAN=true || ORACLE_CLEAN=false

jq -n \
    --arg version "$VERSION" \
    --arg features "$FEATURES" \
    --arg rustc "$(rustc -V)" \
    --arg appimagetool "$(/opt/squashfs-root/AppRun --version 2>/dev/null || echo appimagetool)" \
    --arg git_head "$GIT_HEAD" \
    --arg built_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --argjson data_files "$COPIED_FILES" \
    --argjson data_bytes "$COPIED_BYTES" \
    --argjson data_classes "$DATA_CLASSES" \
    --argjson remaster_files "$REMASTER_FILES" \
    --argjson remaster_bytes "$REMASTER_BYTES" \
    --argjson binary_bytes "$(stat -c %s "$STAGE/max-brh")" \
    --arg binary_sha "$BINARY_SHA" \
    --arg archive_name "$(basename "$ARCHIVE")" \
    --argjson archive_bytes "$ARCHIVE_BYTES" \
    --arg archive_sha "$ARCHIVE_SHA" \
    --argjson tar_gz_bytes "$GZ_SIZE" \
    --argjson tar_xz_bytes "$XZ_SIZE" \
    --arg appimage_name "$(basename "$APPIMAGE")" \
    --argjson appimage_bytes "$APPIMAGE_BYTES" \
    --arg appimage_sha "$APPIMAGE_SHA" \
    --argjson appdir_files "$APPDIR_FILES" \
    --argjson oracle_clean "$ORACLE_CLEAN" \
    '{
        schema: 1,
        version: $version,
        platform: "linux-x86_64",
        portable: true,
        features: $features,
        toolchain: $rustc,
        appimagetool: $appimagetool,
        git_head: $git_head,
        built_at_utc: $built_at,
        data: { files: $data_files, bytes: $data_bytes, hash_verified: true, classes: $data_classes },
        remaster: { files: $remaster_files, bytes: $remaster_bytes, mandatory: true },
        binary: { path: "max-brh", bytes: $binary_bytes, sha256: $binary_sha },
        packages: {
            archive: { name: $archive_name, bytes: $archive_bytes, sha256: $archive_sha,
                       tar_gz_bytes: $tar_gz_bytes, tar_xz_bytes: $tar_xz_bytes },
            appimage: { name: $appimage_name, bytes: $appimage_bytes, sha256: $appimage_sha,
                        appdir_files: $appdir_files }
        },
        oracles_untouched: $oracle_clean
    }' > "$OUT/staging-report.json"

say "done"
cat "$OUT/SHA256SUMS"
echo
echo "staging report: $OUT/staging-report.json"
