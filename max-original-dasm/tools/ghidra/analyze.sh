#!/bin/bash
# Полный анализ MAXRUN.EXE в Ghidra headless + экспорт функций/строк/xrefs.
# Запуск: docker run --rm --entrypoint bash -v "D:/dev/max-port-debug:/src" -w /src max-ghidra \
#          bash /src/max-original-dasm/tools/ghidra/analyze.sh
set -euxo pipefail

PROJ=/src/max-original-dasm/ghidra-project
rm -rf "$PROJ"
mkdir -p "$PROJ" /src/max-original-dasm/export

/opt/ghidra/support/analyzeHeadless "$PROJ" MAX_RE \
    -import /src/max-original-dasm/maxrun.flat \
    -loader BinaryLoader \
    -loader-baseAddr 0x10000 \
    -processor x86:LE:32:default \
    -overwrite \
    -scriptPath /src/max-original-dasm/tools/ghidra \
    -preScript set_entry.py \
    -preScript define_strings.py \
    -postScript export_results.py /src/max-original-dasm/export

ls -la /src/max-original-dasm/export
head -5 /src/max-original-dasm/export/functions.csv
wc -l /src/max-original-dasm/export/functions.csv /src/max-original-dasm/export/strings.csv
