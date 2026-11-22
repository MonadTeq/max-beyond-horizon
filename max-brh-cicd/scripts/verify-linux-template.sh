#!/bin/bash
# Smoke-checks the Linux server template: shared libraries, entry message, exit status.
#
# Invoked as: docker run --rm \
#     -v "D:/dev/max-port-debug:/src:ro" \
#     -v "D:/dev/max-port-debug/bin/linux-server-template:/srv:ro" \
#     ubuntu:24.04 bash /src/build/verify-linux-template.sh /srv
#
# The template is copied to a writable directory first so the smoke run cannot leave
# autosaves or desync reports in the shipped template.
set -u

SRC="${1:-/srv}"
RUN=/tmp/max-template-smoke

rm -rf "$RUN"
mkdir -p "$RUN"
cp -a "$SRC"/. "$RUN"/

cd "$RUN" || exit 1

echo "=== template files ==="
ls -la

echo "=== executable bit ==="
if [ -x ./max-port ]; then echo "max-port is executable"; else echo "max-port is NOT executable"; fi

echo "=== ldd ==="
missing=$(LD_LIBRARY_PATH=. ldd ./max-port 2>/dev/null | grep "not found" || true)
if [ -n "$missing" ]; then
    echo "$missing"
else
    echo "all libraries resolved"
fi

echo "=== smoke run (12s) ==="
timeout 12 env LD_LIBRARY_PATH=. ./max-port --server > server.out 2>&1
rc=$?
echo "exit=$rc (124 means the server was still running when the timeout killed it)"
echo "--- first 40 lines of output ---"
sed -n '1,40p' server.out

echo "=== markers ==="
echo "SERVER: entry count: $(grep -c 'SERVER: entry' server.out)"
grep -E 'SERVER: (entry|initializing resources|resources ready)' server.out || true
