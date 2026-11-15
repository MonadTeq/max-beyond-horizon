#!/bin/bash
# Builds and runs the font reference generator.
#
#   docker run --rm -v "D:/dev/max-port-debug:/src" -w /src/max-rust/tools/font_vectors \
#       rust:1.94.0-bookworm bash run.sh
#
# Outputs (in /src/build/font-vectors): vectors.txt (per-glyph metrics from
# FreeType and from the original .FON), render.txt (shared bitmap renderings),
# and *.bmp for the sample strings.  See README.md.
set -e
cd "$(dirname "$0")"
apt-get update -qq
apt-get install -y -qq g++ libfreetype-dev pkg-config >/dev/null
g++ -O2 -std=c++17 main.cpp -o font_vectors $(pkg-config --cflags --libs freetype2)

out=/src/build/font-vectors
mkdir -p "$out"
cd "$out"
/src/max-rust/tools/font_vectors/font_vectors /src/assets/PATCHES.RES /src/max-original
echo "EXIT=$?"
ls -la
