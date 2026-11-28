#!/usr/bin/env bash
# G4 gate: the packaged Linux distribution, end to end, in clean directories
# (max-brh-docs/development/distro-installers.md §F: "распаковал → запустил →
# сейвы в каталоге игры; AppImage запускается").
#
# Runs INSIDE max-brh-installer/linux/Dockerfile, on the artifacts that
# build-distro.sh wrote to /work/out:
#
#   docker run --rm -v <repo>:/src -v <work>:/work max-brh-linux-dist:0.1.28a \
#       bash /src/max-brh-installer/linux/gate-g4.sh
#
# Checks:
#   1. artifact hashes (SHA256SUMS) and the archive/AppImage presence;
#   2. tar.gz unpacks into one self-contained catalog: data/ = 509 oracle files
#      (size + sha256), mandatory remaster/, empty saves/ and missions/;
#   3. ldd of the unpacked binary: nothing missing, SDL3 comes from lib/ (rpath);
#   4. headless run from a foreign CWD: --new-game reads data/ (1828 MAX.RES
#      entries, 84 PATCHES.RES), --demo reads a save from saves/;
#   5. GUI under Xvfb: the window opens and preferences.ini is read from the
#      catalog — the same game_root that holds saves/ (layout.rs);
#   6. the AppImage extracts (--appimage-extract): the same data + remaster
#      inside, RPATH usr/lib, clean ldd;
#   7. the AppImage starts under Xvfb (APPIMAGE_EXTRACT_AND_RUN=1, no FUSE in a
#      container): a window appears and the writable root is the directory of
#      the .AppImage — saves/, missions/ and settings.jsonc appear next to it,
#      the player's preferences.ini is read from there;
#   8. the oracles are untouched and nothing was written outside the catalogs.
set -uo pipefail

SRC="${SRC:-/src}"
WORK="${WORK:-/work}"
OUT="$WORK/out"
G4="$WORK/g4"
LOG="$G4/g4.log"
INV="$SRC/max-brh-installer/data-inventory.json"

rm -rf "$G4"
mkdir -p "$G4"
exec > >(tee -a "$LOG") 2>&1

PASS=0
FAIL=0
ok() { PASS=$((PASS + 1)); printf '  PASS  %s\n' "$1"; }
bad() { FAIL=$((FAIL + 1)); printf '  FAIL  %s\n' "$1"; }
section() { printf '\n== %s ==\n' "$1"; }

# wait_for <fixed string> <file> <tenths of a second>
wait_for() {
    local pattern="$1" file="$2" ticks="$3" i=0
    while [ "$i" -lt "$ticks" ]; do
        if grep -qF -- "$pattern" "$file" 2>/dev/null; then return 0; fi
        sleep 0.1
        i=$((i + 1))
    done
    return 1
}

# wait_window <display> <name regex> <tenths of a second>
wait_window() {
    local display="$1" name="$2" ticks="$3" i=0
    while [ "$i" -lt "$ticks" ]; do
        if DISPLAY="$display" xdotool search --onlyvisible --name "$name" >/dev/null 2>&1; then
            return 0
        fi
        sleep 0.1
        i=$((i + 1))
    done
    return 1
}

# quit_window <display> <name regex> — Esc (the menu's Quit) or SIGTERM fallback.
quit_window() {
    local display="$1" name="$2" pid="$3" wid
    wid="$(DISPLAY="$display" xdotool search --onlyvisible --name "$name" 2>/dev/null | head -n 1)"
    if [ -n "$wid" ]; then
        DISPLAY="$display" xdotool windowfocus "$wid" 2>/dev/null || true
        DISPLAY="$display" xdotool key --window "$wid" Escape 2>/dev/null || true
    fi
    for _ in $(seq 1 50); do
        kill -0 "$pid" 2>/dev/null || return 0
        sleep 0.2
    done
    kill -TERM "$pid" 2>/dev/null || true
    sleep 1
    kill -KILL "$pid" 2>/dev/null || true
    return 0
}

if [ ! -f "$INV" ]; then
    printf 'gate-g4: missing %s\n' "$INV"
    exit 1
fi

EXPECTED_FILES="$(jq -r '.totals.files' "$INV")"
EXPECTED_BYTES="$(jq -r '.totals.bytes' "$INV")"

# --- 1. Artifacts ------------------------------------------------------------------
section "1/8 artifacts"
ARCHIVE="$(find "$OUT" -maxdepth 1 \( -name 'max-brh-*-linux.tar.gz' -o -name 'max-brh-*-linux.tar.xz' \) | sort | head -n 1)"
APPIMAGE="$(find "$OUT" -maxdepth 1 -name 'MAX-BRH-*-x86_64.AppImage' | sort | head -n 1)"
if [ -n "$ARCHIVE" ]; then
    ok "archive: $(basename "$ARCHIVE") ($(stat -c %s "$ARCHIVE") bytes)"
else
    bad "no tar.gz/tar.xz archive in $OUT"
fi
if [ -n "$APPIMAGE" ]; then
    ok "AppImage: $(basename "$APPIMAGE") ($(stat -c %s "$APPIMAGE") bytes)"
else
    bad "no AppImage in $OUT"
fi
if [ -f "$OUT/SHA256SUMS" ] && [ -n "$ARCHIVE" ] && [ -n "$APPIMAGE" ]; then
    if ( cd "$OUT" && sha256sum -c --quiet SHA256SUMS ); then
        ok "SHA256SUMS matches the archive and the AppImage"
    else
        bad "SHA256SUMS does not match the artifacts"
    fi
else
    bad "missing $OUT/SHA256SUMS"
fi
[ -n "$ARCHIVE" ] && [ -n "$APPIMAGE" ] || {
    printf '\ngate-g4: no artifacts to test\n'
    exit 1
}
VERSION="$(basename "$ARCHIVE" | sed -E 's/^max-brh-(.*)-linux\.tar\.(gz|xz)$/\1/')"

# --- 2. tar.gz: unpack and layout --------------------------------------------------
section "2/8 archive: unpack + portable layout"
rm -rf "$G4/tar"
mkdir -p "$G4/tar"
case "$ARCHIVE" in
    *.tar.gz) tar -xzf "$ARCHIVE" -C "$G4/tar" ;;
    *.tar.xz) tar -xJf "$ARCHIVE" -C "$G4/tar" ;;
esac
CATALOG="$(find "$G4/tar" -maxdepth 1 -mindepth 1 -type d | sort | head -n 1)"
if [ -n "$CATALOG" ] && [ "$(basename "$CATALOG")" = "max-brh-$VERSION-linux" ]; then
    ok "unpacked: $(basename "$CATALOG")/"
else
    bad "unpacked catalog is $(basename "${CATALOG:-<none>}")/, expected max-brh-$VERSION-linux/"
fi

DATA_FILES="$(find "$CATALOG/data" -type f 2>/dev/null | wc -l)"
DATA_BYTES="$(find "$CATALOG/data" -type f -printf '%s\n' 2>/dev/null | awk '{ s += $1 } END { print s + 0 }')"
if [ "$DATA_FILES" = "$EXPECTED_FILES" ]; then
    ok "data/: $DATA_FILES files (data-inventory.json)"
else
    bad "data/: $DATA_FILES files, expected $EXPECTED_FILES"
fi
if [ "$DATA_BYTES" = "$EXPECTED_BYTES" ]; then
    ok "data/: $DATA_BYTES bytes (data-inventory.json)"
else
    bad "data/: $DATA_BYTES bytes, expected $EXPECTED_BYTES"
fi
for name in MAX.RES PATCHES.RES COLOR.PAL; do
    want="$(jq -r --arg p "$name" '.files[] | select(.path == $p) | .sha256' "$INV")"
    got="$(sha256sum "$CATALOG/data/$name" 2>/dev/null | cut -d' ' -f1)"
    if [ -n "$want" ] && [ "$want" = "$got" ]; then
        ok "data/$name sha256 = oracle inventory"
    else
        bad "data/$name sha256 mismatch (want ${want:-?}, got ${got:-missing})"
    fi
done
if ( cd "$CATALOG" && sha256sum -c --quiet SHA256SUMS >/dev/null 2>&1 ); then
    ok "SHA256SUMS: every packaged file matches its hash"
else
    bad "SHA256SUMS: a packaged file does not match its hash"
fi
if [ -f "$CATALOG/remaster/remaster-manifest.json" ] \
    && [ "$(find "$CATALOG/remaster" -maxdepth 1 -name '*.ktx2' | wc -l)" -gt 0 ]; then
    ok "remaster/: manifest + $(find "$CATALOG/remaster" -type f | wc -l) mandatory files"
else
    bad "remaster/ is missing its manifest or the .ktx2 layers"
fi
if [ -d "$CATALOG/saves" ] && [ -z "$(ls -A "$CATALOG/saves")" ] \
    && [ -d "$CATALOG/missions" ] && [ -z "$(ls -A "$CATALOG/missions")" ]; then
    ok "saves/ and missions/ exist empty (created by the package)"
else
    bad "saves/ and missions/ are missing or not empty"
fi
MISSING_DOCS=""
for file in LICENSE THIRD_PARTY_LICENSES.txt README.txt CHANGELOG.md settings.jsonc run.sh; do
    [ -e "$CATALOG/$file" ] || MISSING_DOCS="$MISSING_DOCS $file"
done
if [ -z "$MISSING_DOCS" ]; then
    ok "LICENSE, THIRD_PARTY_LICENSES.txt, README.txt, CHANGELOG.md, settings.jsonc, run.sh"
else
    bad "missing package files:$MISSING_DOCS"
fi
if [ -x "$CATALOG/max-brh" ] && head -c 4 "$CATALOG/max-brh" | od -An -tx1 | tr -d ' \n' | grep -q '7f454c46'; then
    ok "max-brh is an executable ELF"
else
    bad "max-brh is missing or not executable"
fi

# --- 3. ldd ------------------------------------------------------------------------
section "3/8 ldd: no missing libraries, SDL3 from the package"
NOT_FOUND="$(env -u LD_LIBRARY_PATH ldd "$CATALOG/max-brh" 2>/dev/null | grep -c 'not found' || true)"
if [ "$NOT_FOUND" = "0" ]; then
    ok "ldd: every library resolved"
else
    env -u LD_LIBRARY_PATH ldd "$CATALOG/max-brh" | grep 'not found' | sed 's/^/        /'
    bad "ldd: $NOT_FOUND unresolved libraries"
fi
SDL_PATH="$(env -u LD_LIBRARY_PATH ldd "$CATALOG/max-brh" 2>/dev/null | awk '$1 ~ /^libSDL3/ { print $3 }')"
case "$SDL_PATH" in
    "$CATALOG"/lib/*) ok "libSDL3 resolved from the package: ${SDL_PATH#"$CATALOG"/}" ;;
    "") bad "libSDL3 is not a dynamic dependency of the binary" ;;
    *) bad "libSDL3 resolved outside the package: $SDL_PATH" ;;
esac
if readelf -d "$CATALOG/max-brh" | grep -Eq '(RUNPATH|RPATH).*\$ORIGIN/lib'; then
    ok "RPATH \$ORIGIN/lib (the catalog runs from any directory)"
else
    bad "the binary has no \$ORIGIN/lib rpath"
fi

# --- 4. headless run from a foreign CWD --------------------------------------------
section "4/8 headless run from a foreign CWD"
mkdir -p "$G4/smoke" "$G4/frames"
cp "$SRC/max-original/SAVE1.CAM" "$CATALOG/saves/SAVE1.CAM"

DEMO_RC=0
( cd "$G4/smoke" && "$CATALOG/max-brh" --demo "$CATALOG/saves/SAVE1.CAM" --turns 3 > "$G4/demo.log" 2>&1 ) || DEMO_RC=$?
if [ "$DEMO_RC" = "0" ] \
    && grep -qE 'demo: save=.*turns=3 units=[0-9]+' "$G4/demo.log" \
    && grep -q 'state_digest=0x' "$G4/demo.log" \
    && grep -q 'state_checksum=0x' "$G4/demo.log"; then
    ok "--demo reads saves/SAVE1.CAM: $(grep -m1 'demo: ' "$G4/demo.log")"
else
    bad "--demo failed (exit $DEMO_RC): $(tail -n 3 "$G4/demo.log" | tr '\n' ' ')"
fi

NG_RC=0
( cd "$G4/smoke" && "$CATALOG/max-brh" --new-game --out "$G4/frames/frame.ppm" > "$G4/newgame.log" 2>&1 ) || NG_RC=$?
if [ "$NG_RC" = "0" ] && [ -s "$G4/frames/frame.ppm" ] && [ "$(stat -c %s "$G4/frames/frame.ppm")" -gt 100000 ]; then
    ok "--new-game renders a frame from the packaged data ($(stat -c %s "$G4/frames/frame.ppm") bytes PPM)"
else
    bad "--new-game failed (exit $NG_RC): $(tail -n 3 "$G4/newgame.log" | tr '\n' ' ')"
fi
if grep -q 'MAX.RES=1828' "$G4/newgame.log" && grep -q 'PATCHES.RES=84' "$G4/newgame.log"; then
    ok "data/ read by the game: $(grep -m1 '^assets: ' "$G4/newgame.log")"
else
    bad "unexpected asset report: $(grep -m1 '^assets: ' "$G4/newgame.log" || echo '<none>')"
fi
if grep -q 'PATCHES.RES грузится из fallback' "$G4/newgame.log"; then
    if grep -q "fallback $CATALOG/data/PATCHES.RES" "$G4/newgame.log"; then
        ok "the default assets/PATCHES.RES path falls back to data/PATCHES.RES inside the catalog"
    else
        bad "PATCHES.RES fallback resolved outside the catalog: $(grep -m1 'PATCHES.RES грузится' "$G4/newgame.log")"
    fi
fi
# The package ships the oracle data byte-for-byte, so the only notices are the
# original archive's own gaps (MASTER/S_SUBMAR/F_MASTER): no file must be missing
# from the package.
if grep -qE 'No such file|не удалось прочитать|Permission denied' "$G4/newgame.log"; then
    bad "file errors while reading the packaged data: $(grep -m1 -E 'No such file|не удалось прочитать|Permission denied' "$G4/newgame.log")"
else
    ok "no file errors; $(grep -c '^  warning: ' "$G4/newgame.log" || true) known oracle-data notices (printed in the log)"
    grep '^  warning: ' "$G4/newgame.log" | sed 's/^/        /'
fi
if [ -z "$(ls -A "$G4/smoke")" ]; then
    ok "nothing written into the foreign CWD"
else
    bad "the foreign CWD gained files: $(ls -A "$G4/smoke" | tr '\n' ' ')"
fi

# --- 5. GUI of the catalog under Xvfb ----------------------------------------------
section "5/8 GUI of the catalog under Xvfb"
printf 'music=0.5\nfx=0.5\n' > "$CATALOG/preferences.ini"
Xvfb :91 -screen 0 1280x1024x24 -nolisten tcp > "$G4/xvfb-catalog.log" 2>&1 &
XVFB_PID=$!
sleep 1

( cd "$G4/smoke" && exec env DISPLAY=:91 SDL_AUDIODRIVER=dummy "$CATALOG/max-brh" --ui --scale 1 ) \
    > "$G4/ui-catalog.log" 2>&1 &
UI_PID=$!
if wait_for "настройки загружены из" "$G4/ui-catalog.log" 300; then
    if grep -qF "$CATALOG/preferences.ini" "$G4/ui-catalog.log"; then
        ok "game_root = the catalog: preferences.ini read from $(basename "$CATALOG")/"
    else
        bad "preferences.ini read from elsewhere: $(grep -m1 'настройки загружены из' "$G4/ui-catalog.log")"
    fi
else
    bad "the UI did not report loading preferences: $(tail -n 5 "$G4/ui-catalog.log" | tr '\n' ' ')"
fi
if wait_window :91 'M.A.X' 300; then
    ok "SDL window created under Xvfb: $(DISPLAY=:91 xdotool search --onlyvisible --name 'M.A.X' | head -n 1)"
else
    bad "no SDL window under Xvfb: $(tail -n 5 "$G4/ui-catalog.log" | tr '\n' ' ')"
fi
quit_window :91 'M.A.X' "$UI_PID"
wait "$UI_PID" 2>/dev/null
if [ "$(ls -A "$CATALOG/saves" | tr '\n' ' ')" = "SAVE1.CAM " ]; then
    ok "the catalog saves/ holds only the test save (the UI run did not scatter files)"
else
    bad "unexpected files in the catalog saves/: $(ls -A "$CATALOG/saves" | tr '\n' ' ')"
fi
if [ -z "$(ls -A "$G4/smoke")" ]; then
    ok "the GUI wrote nothing into the foreign CWD"
else
    bad "the GUI wrote into the foreign CWD: $(ls -A "$G4/smoke" | tr '\n' ' ')"
fi
kill "$XVFB_PID" 2>/dev/null
wait "$XVFB_PID" 2>/dev/null

# --- 6. AppImage: extract ----------------------------------------------------------
section "6/8 AppImage: extract + layout"
APP_PLAYER="$G4/appimage"
mkdir -p "$APP_PLAYER/extract" "$APP_PLAYER/foreign" "$G4/appimage-tmp"
APPFILE="$APP_PLAYER/$(basename "$APPIMAGE")"
cp "$APPIMAGE" "$APPFILE"
chmod 755 "$APPFILE"
( cd "$APP_PLAYER/extract" && "$APPFILE" --appimage-extract > "$G4/appimage-extract.log" 2>&1 )
EXTRACT_RC=$?
ROOT="$APP_PLAYER/extract/squashfs-root"
if [ "$EXTRACT_RC" = "0" ] && [ -x "$ROOT/usr/bin/max-brh" ]; then
    ok "--appimage-extract: squashfs-root/usr/bin/max-brh"
else
    bad "--appimage-extract failed (exit $EXTRACT_RC): $(tail -n 3 "$G4/appimage-extract.log" | tr '\n' ' ')"
fi
APP_DATA_FILES="$(find "$ROOT/data" -type f 2>/dev/null | wc -l)"
if [ "$APP_DATA_FILES" = "$EXPECTED_FILES" ]; then
    ok "AppDir data/: $APP_DATA_FILES files (the same oracle data)"
else
    bad "AppDir data/: $APP_DATA_FILES files, expected $EXPECTED_FILES"
fi
if [ -f "$ROOT/remaster/remaster-manifest.json" ] && [ -x "$ROOT/AppRun" ] \
    && [ -f "$ROOT/max-brh.desktop" ] && [ -e "$ROOT/.DirIcon" ]; then
    ok "AppDir: AppRun, max-brh.desktop, .DirIcon/icon, remaster/ inside"
else
    bad "AppDir is missing AppRun/.desktop/.DirIcon/remaster"
fi
for name in MAX.RES PATCHES.RES COLOR.PAL; do
    want="$(jq -r --arg p "$name" '.files[] | select(.path == $p) | .sha256' "$INV")"
    got="$(sha256sum "$ROOT/data/$name" 2>/dev/null | cut -d' ' -f1)"
    if [ -n "$want" ] && [ "$want" = "$got" ]; then
        ok "AppDir data/$name sha256 = oracle inventory"
    else
        bad "AppDir data/$name sha256 mismatch (want ${want:-?}, got ${got:-missing})"
    fi
done
APP_NOT_FOUND="$(env -u LD_LIBRARY_PATH ldd "$ROOT/usr/bin/max-brh" 2>/dev/null | grep -c 'not found' || true)"
if [ "$APP_NOT_FOUND" = "0" ]; then
    ok "AppDir ldd: every library resolved (usr/lib)"
else
    env -u LD_LIBRARY_PATH ldd "$ROOT/usr/bin/max-brh" | grep 'not found' | sed 's/^/        /'
    bad "AppDir ldd: $APP_NOT_FOUND unresolved libraries"
fi
if readelf -d "$ROOT/usr/bin/max-brh" | grep -Eq '(RUNPATH|RPATH).*\$ORIGIN/\.\./lib'; then
    ok "AppDir RPATH \$ORIGIN/../lib"
else
    bad "the AppImage binary has no \$ORIGIN/../lib rpath"
fi

# --- 7. AppImage: run under Xvfb ---------------------------------------------------
section "7/8 AppImage: run under Xvfb (no FUSE: APPIMAGE_EXTRACT_AND_RUN=1)"
printf 'music=0.5\nfx=0.5\n' > "$APP_PLAYER/preferences.ini"
rm -rf "$APP_PLAYER/saves" "$APP_PLAYER/missions" "$APP_PLAYER/settings.jsonc"
Xvfb :92 -screen 0 1280x1024x24 -nolisten tcp > "$G4/xvfb-appimage.log" 2>&1 &
XVFB2_PID=$!
sleep 1

( cd "$APP_PLAYER/foreign" && exec env APPIMAGE_EXTRACT_AND_RUN=1 DISPLAY=:92 \
    SDL_AUDIODRIVER=dummy TMPDIR="$G4/appimage-tmp" \
    "$APPFILE" --scale 1 ) > "$G4/appimage-ui.log" 2>&1 &
AI_PID=$!
if wait_for "настройки загружены из" "$G4/appimage-ui.log" 600; then
    if grep -qF "$APP_PLAYER/preferences.ini" "$G4/appimage-ui.log"; then
        ok "game_root = the .AppImage directory: preferences.ini read from next to the file"
    else
        bad "preferences.ini read from elsewhere: $(grep -m1 'настройки загружены из' "$G4/appimage-ui.log")"
    fi
else
    bad "the AppImage did not report loading preferences: $(tail -n 8 "$G4/appimage-ui.log" | tr '\n' ' ')"
fi
if wait_window :92 'M.A.X' 600; then
    ok "AppImage window created under Xvfb: $(DISPLAY=:92 xdotool search --onlyvisible --name 'M.A.X' | head -n 1)"
else
    bad "no AppImage window under Xvfb: $(tail -n 8 "$G4/appimage-ui.log" | tr '\n' ' ')"
fi
if [ -d "$APP_PLAYER/saves" ] && [ -d "$APP_PLAYER/missions" ]; then
    ok "saves/ and missions/ created next to the .AppImage"
else
    bad "AppRun did not create saves/ and missions/ next to the .AppImage"
fi
if [ -f "$APP_PLAYER/settings.jsonc" ]; then
    ok "settings.jsonc seeded next to the .AppImage (editable copy of the template)"
else
    bad "settings.jsonc was not seeded next to the .AppImage"
fi
quit_window :92 'M.A.X' "$AI_PID"
wait "$AI_PID" 2>/dev/null
if [ -z "$(ls -A "$APP_PLAYER/foreign")" ]; then
    ok "the AppImage wrote nothing into the player's foreign CWD"
else
    bad "the AppImage wrote into the foreign CWD: $(ls -A "$APP_PLAYER/foreign" | tr '\n' ' ')"
fi
kill "$XVFB2_PID" 2>/dev/null
wait "$XVFB2_PID" 2>/dev/null

# --- 8. Oracles untouched ----------------------------------------------------------
section "8/8 oracles and reproducibility"
ORACLE_DIRTY="$(git -C "$SRC" status --porcelain -- max-original max-cpp-port max-original-dasm 2>/dev/null)"
if [ -z "$ORACLE_DIRTY" ]; then
    ok "git status clean for max-original/, max-cpp-port/, max-original-dasm/"
else
    bad "the oracles are modified: $ORACLE_DIRTY"
fi
if ( cd "$CATALOG" && cmp -s "data/MAX.RES" "$SRC/max-original/MAX.RES" ); then
    ok "data/MAX.RES is byte-identical to the oracle file"
else
    bad "data/MAX.RES differs from the oracle file"
fi

# --- Summary -----------------------------------------------------------------------
section "summary"
printf 'G4: %d passed, %d failed\n' "$PASS" "$FAIL"
jq -n --arg version "$VERSION" \
    --argjson passed "$PASS" --argjson failed "$FAIL" \
    --arg archive "$(basename "$ARCHIVE")" \
    --argjson archive_bytes "$(stat -c %s "$ARCHIVE")" \
    --arg appimage "$(basename "$APPIMAGE")" \
    --argjson appimage_bytes "$(stat -c %s "$APPIMAGE")" \
    --argjson data_files "$APP_DATA_FILES" \
    --argjson data_bytes "$EXPECTED_BYTES" \
    '{schema: 1, gate: "G4", version: $version, platform: "linux-x86_64",
      passed: $passed, failed: $failed,
      archive: {name: $archive, bytes: $archive_bytes},
      appimage: {name: $appimage, bytes: $appimage_bytes},
      data: {files: $data_files, bytes: $data_bytes},
      checks_log: "g4.log"}' > "$G4/g4-summary.json"

[ "$FAIL" -eq 0 ]
