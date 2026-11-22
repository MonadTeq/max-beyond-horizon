#!/bin/bash
# Prepares bin/linux-server-template (headless + shared libs + game data) and smoke-runs it.
set -x

rm -rf /src/bin/linux-server-template
mkdir -p /src/bin/linux-server-template

cp /src/build/linux-x64-debug/max-port /src/bin/linux-server-template/

for lib in libSDL3.so.0 libutf8proc.so.3 libminiaudio.so libnlohmann_json_schema_validator.so.2 liblua.so libfreetype.so.6 libminiupnpc.so.21; do
    find /src/build/linux-x64-debug/_deps -name "$lib" -exec cp -L {} /src/bin/linux-server-template/ \;
done

for f in .portable PATCHES.RES settings.json language.json help.json maxint.json LICENSE; do
    cp /src/build/server-run/$f /src/bin/linux-server-template/ 2>/dev/null || true
done

ls -la /src/bin/linux-server-template/

echo "=== ldd from template ==="
cd /src/bin/linux-server-template
LD_LIBRARY_PATH=. ldd ./max-port | grep "not found" || echo "all libraries resolved"

echo "=== smoke run ==="
timeout 12 env LD_LIBRARY_PATH=. ./max-port --server 2>&1 | head -20
echo "RUN_EXIT=$?"
