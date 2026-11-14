#!/bin/bash
# r2_export.sh - экспорт функций и строк MAXRUN.EXE из radare2 в JSON.
#
# Запуск в контейнере ubuntu:24.04 (radare2 уже стоит или поставится):
#   docker run --rm --entrypoint bash \
#       -v "D:/dev/max-port-debug:/src" -w /src ubuntu:24.04 \
#       /src/max-original-dasm/tools/r2_export.sh
#
# Результат (в /src/max-original-dasm/export/):
#   r2_functions.json  - вывод `aflj` (все функции)
#   r2_strings.json    - [{addr, text, xrefs:[...]}] по строкам из
#                        string-matches.csv
#
# Два важных момента по данным (проверено на этом образе):
#
# 1) В string-matches.csv колонка flat_offset - это ФАЙЛОВЫЙ оффсет
#    (paddr - 0x3AB00, см. tools/match_strings.py), а не оффсет в
#    maxrun.flat. Литеральная формула addr = 0x10000 + flat_offset
#    проверяется по байтам образа; если не сходится, адрес берётся из
#    01-strings.txt (колонка vaddr) - это тот же источник, из которого
#    сделан CSV. Последний резерв - уникальный поиск строки в образе.
#
# 2) В maxrun.flat LE-фиксапы (relocations) не применены, поэтому r2 не
#    видит ссылок на строки и `axtj` возвращает []. Если рядом лежит
#    max-original/MAXRUN.EXE, скрипт делает копию образа в /tmp с
#    применёнными 32-битными fixup'ами и анализирует её: база (0x10000),
#    адреса и размеры те же, но ссылки на строки становятся видны.
#    Отключить: R2_NO_RELOC=1 (тогда анализируется сам maxrun.flat).
#
# Внутри - один запуск r2: `aaa`, `aflj`, определения строк (`s`/`Cs`) и
# `axtj @ <addr>` по каждому адресу; поток JSON разбирается jq.
set -euxo pipefail

DASM_DIR=/src/max-original-dasm
FLAT="$DASM_DIR/maxrun.flat"
MATCHES="$DASM_DIR/string-matches.csv"
STRINGS_TXT="$DASM_DIR/01-strings.txt"
EXE="$DASM_DIR/../max-original/MAXRUN.EXE"
EXPORT="$DASM_DIR/export"

BASE=0x10000          # база плоского образа
CSV_FILE_BASE=0x3AB00 # FLAT_BASE из tools/match_strings.py (paddr - flat_offset)
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# --- зависимости (в ubuntu:24.04 radare2 обычно уже установлен) -------------
apt_install() {
    apt-get update
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@"
}
if ! command -v r2 >/dev/null 2>&1; then
    apt_install radare2
fi
if ! command -v python3 >/dev/null 2>&1; then
    apt_install python3
fi
if ! command -v jq >/dev/null 2>&1; then
    apt_install jq
fi

if [ ! -f "$FLAT" ]; then
    echo "нет плоского образа: $FLAT" >&2
    exit 1
fi
if [ ! -f "$MATCHES" ]; then
    echo "нет списка строк: $MATCHES" >&2
    exit 1
fi
mkdir -p "$EXPORT"

# --- 1. копия образа с применёнными LE-фиксапами ---------------------------
# maxrun.flat содержит неразрешённые ссылки: в коде лежат target-оффсеты из
# таблицы fixup'ов, а не готовые адреса. Патчим только type 7 (32-bit offset):
# value = object_base + target_offset. Исходные файлы не меняются.
IMAGE="$FLAT"
if [ "${R2_NO_RELOC:-0}" != "1" ] && [ -f "$EXE" ]; then
    if python3 - "$EXE" "$FLAT" "$TMP/maxrun.reloc.flat" <<'PY'
import struct
import sys

exe_path, flat_path, out_path = sys.argv[1], sys.argv[2], sys.argv[3]
data = open(exe_path, "rb").read()
if data[:2] != b"MZ":
    raise SystemExit("not an MZ file")
le = struct.unpack_from("<I", data, 0x3C)[0]
if data[le:le + 2] != b"LE":
    raise SystemExit("not an LE file")

u16 = lambda o: struct.unpack_from("<H", data, o)[0]
u32 = lambda o: struct.unpack_from("<I", data, o)[0]
i16 = lambda o: struct.unpack_from("<h", data, o)[0]

obj_table = le + u32(le + 0x40)
obj_count = u32(le + 0x44)
bases = []
for index in range(obj_count):
    entry = obj_table + index * 24
    _vsize, base, _flags, _first_page, _pages, _res = struct.unpack_from(
        "<IIIIII", data, entry
    )
    bases.append(base & 0xFFFFFFFF)

page_count = u32(le + 0x14)
fixup_page_table = le + u32(le + 0x68)
fixup_records = le + u32(le + 0x6C)
entries = [u32(fixup_page_table + index * 4) for index in range(page_count + 1)]

flat = bytearray(open(flat_path, "rb").read())
patched = 0
for page in range(1, page_count + 1):
    cursor = fixup_records + entries[page - 1]
    end = fixup_records + entries[page]
    while cursor < end:
        word = u16(cursor)
        source_type = word & 0xF
        flags = word & 0xFFF0
        source_offset = i16(cursor + 2)
        # 0x4000 - 16-битный номер объекта, иначе 1 байт; 0x1000 - 32-битный
        # target offset, иначе 16-битный (так это разбирает DOS/32A loader.asm).
        if flags & 0x4000:
            obj = u16(cursor + 4)
            pos = cursor + 6
        else:
            obj = data[cursor + 4]
            pos = cursor + 5
        if flags & 0x1000:
            target = u32(pos)
            pos += 4
        else:
            target = u16(pos)
            pos += 2
        if pos <= cursor or pos > end:
            raise SystemExit("fixup record parse error at 0x%x" % cursor)
        # Импорты/цепочки (0x0F20) и чужие объекты не трогаем.
        if source_type == 7 and not (flags & 0x0F20) and 1 <= obj <= len(bases):
            offset = (page - 1) * 0x1000 + (source_offset & 0xFFFF)
            if 0 <= offset <= len(flat) - 4:
                flat[offset:offset + 4] = struct.pack(
                    "<I", (bases[obj - 1] + target) & 0xFFFFFFFF
                )
                patched += 1
        cursor = pos

if patched < 1000:
    raise SystemExit("too few fixups: %d" % patched)
open(out_path, "wb").write(flat)
print("LE fixups applied: %d" % patched, file=sys.stderr)
PY
    then
        IMAGE="$TMP/maxrun.reloc.flat"
    else
        echo "warning: не удалось применить LE-фиксапы, анализирую исходный maxrun.flat" >&2
    fi
else
    echo "warning: релокации не применяются (R2_NO_RELOC=1 или нет $EXE);" >&2
    echo "         axtj, скорее всего, не найдёт ссылок на строки" >&2
fi

# --- 2. строки: адреса + r2-команды определений ----------------------------
# python3 читает string-matches.csv (там есть запятые/кавычки, поэтому
# разбор через csv, а не sed/awk) и пишет:
#   strdefs.r2      - команды "s <addr>" / "Cs <len>"
#   addresses.tsv   - <addr>\t<base64 текста> для bash-цикла с axtj
python3 - "$MATCHES" "$FLAT" "$STRINGS_TXT" "$BASE" "$CSV_FILE_BASE" \
    "$TMP/strdefs.r2" "$TMP/addresses.tsv" <<'PY'
import base64
import csv
import os
import sys

(matches_path, flat_path, strings_txt, base_s, file_base_s,
 defs_path, addrs_path) = sys.argv[1:8]
base = int(base_s, 0)
file_base = int(file_base_s, 0)
flat = open(flat_path, "rb").read()

# Таблица paddr -> vaddr из 01-strings.txt (её колонки: nth paddr vaddr ...).
paddr_to_vaddr = {}
if os.path.isfile(strings_txt):
    with open(strings_txt, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            parts = line.split()
            if len(parts) < 3:
                continue
            try:
                paddr_to_vaddr[int(parts[1], 16)] = int(parts[2], 16)
            except ValueError:
                continue

defs = []
addresses = []
seen = set()
literal = mapped = searched = skipped = 0

with open(matches_path, "r", encoding="utf-8-sig", newline="") as handle:
    for row in csv.DictReader(handle):
        raw_offset = (row.get("flat_offset") or "").strip()
        text = row.get("string") or ""
        if not raw_offset or not text:
            continue
        try:
            offset = int(raw_offset, 10)
        except ValueError:
            skipped += 1
            continue
        raw = text.encode("utf-8")
        if not raw:
            continue

        # 1) буквальная формула из постановки; принимаем, только если по
        #    адресу действительно лежит эта строка.
        address = base + offset
        img = address - base
        if 0 <= img <= len(flat) - len(raw) and flat[img:img + len(raw)] == raw:
            literal += 1
        else:
            # 2) flat_offset - файловый оффсет; настоящий адрес даёт vaddr.
            vaddr = paddr_to_vaddr.get(offset + file_base)
            if vaddr is not None and 0 <= vaddr - base <= len(flat) - len(raw):
                address = vaddr
                mapped += 1
            else:
                # 3) резерв: строка должна встречаться ровно один раз.
                pos = flat.find(raw)
                if pos < 0 or flat.find(raw, pos + 1) >= 0:
                    skipped += 1
                    continue
                address = base + pos
                searched += 1

        if address in seen:
            continue
        seen.add(address)
        defs.append("s 0x%x" % address)
        defs.append("Cs %d" % len(raw))
        addresses.append(
            "0x%x\t%s" % (address, base64.b64encode(raw).decode("ascii"))
        )

with open(defs_path, "w", encoding="utf-8", newline="\n") as handle:
    handle.write("\n".join(defs) + ("\n" if defs else ""))
with open(addrs_path, "w", encoding="utf-8", newline="\n") as handle:
    handle.write("\n".join(addresses) + ("\n" if addresses else ""))

print("strings: %d (literal=%d, vaddr=%d, search=%d, skipped=%d)"
      % (len(defs) // 2, literal, mapped, searched, skipped), file=sys.stderr)
if skipped:
    print("warning: %d строк не удалось разрешить по адресу" % skipped, file=sys.stderr)
PY

# --- 3. bash-цикл по адресам: маркер + axtj --------------------------------
# xtrace выключаем только на цикл, иначе лог раздувается на 2583 строки.
: > "$TMP/axtj.r2"
set +x
while IFS=$'\t' read -r addr text_b64; do
    if [ -z "$addr" ]; then
        continue
    fi
    # Маркер - валидный JSON-объект, поэтому поток можно разобрать jq.
    printf '?e {"m":"%s","b64":"%s"}\n' "$addr" "$text_b64" >> "$TMP/axtj.r2"
    printf 'axtj @ %s\n' "$addr" >> "$TMP/axtj.r2"
done < "$TMP/addresses.tsv"
set -x

# --- 4. один прогон r2: aaa + aflj + строки + axtj -------------------------
{
    printf 'aaa\n'
    printf 'aflj\n'
    cat "$TMP/strdefs.r2"
    cat "$TMP/axtj.r2"
    printf 'q\n'
} > "$TMP/analyze.r2"

r2 -q -a x86 -b 32 -m "$BASE" \
    -e anal.jmp.tbl=true \
    -e anal.hasnext=true \
    -e anal.datarefs=true \
    -e scr.color=0 \
    -i "$TMP/analyze.r2" \
    "$IMAGE" > "$TMP/stream.json" 2> "$TMP/r2.log"

if [ ! -s "$TMP/stream.json" ]; then
    echo "r2 не вернул JSON; хвост лога:" >&2
    tail -20 "$TMP/r2.log" >&2 || true
    exit 1
fi

# --- 5. разбор потока: [aflj, маркер, axtj, маркер, axtj, ...] -------------
jq -s '.[0]' "$TMP/stream.json" > "$EXPORT/r2_functions.json"

jq -s '
  reduce .[1:][] as $value ({marker: null, out: []};
    if ($value | type) == "object" and ($value.m? != null) then
      .marker = $value
    elif .marker != null then
      .out += [{
        addr: .marker.m,
        text: (.marker.b64 | @base64d),
        xrefs: $value
      }]
      | .marker = null
    else . end)
  | .out
' "$TMP/stream.json" > "$EXPORT/r2_strings.json"

EXPECTED="$(wc -l < "$TMP/addresses.tsv" | tr -d ' ')"
FUNCTIONS="$(jq 'length' "$EXPORT/r2_functions.json")"
STRINGS="$(jq 'length' "$EXPORT/r2_strings.json")"

if [ "$FUNCTIONS" -lt 1 ]; then
    echo "aflj пуст - анализ не дал функций" >&2
    exit 1
fi
if [ "$STRINGS" -ne "$EXPECTED" ]; then
    echo "рассинхрон: адресов строк $EXPECTED, записей в r2_strings.json $STRINGS" >&2
    exit 1
fi
if [ "$EXPECTED" -eq 0 ]; then
    echo "warning: не удалось разрешить ни одной строки" >&2
fi

echo "r2 export: functions=$FUNCTIONS, strings=$STRINGS, image=$IMAGE"
ls -l "$EXPORT/r2_functions.json" "$EXPORT/r2_strings.json"
echo "далее: python3 $DASM_DIR/tools/r2_to_csv.py"
