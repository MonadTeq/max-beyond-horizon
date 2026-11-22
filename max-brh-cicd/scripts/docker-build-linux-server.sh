#!/bin/bash
# Builds the Linux x64 headless dedicated server (MAX_HEADLESS) inside Docker.
# Invoked as: docker run --rm -v "D:/dev/max-port-debug:/src" -w /src max-port-build:latest bash /src/build/docker-build-linux-server.sh
set -euxo pipefail

export DEBIAN_FRONTEND=noninteractive

if ! command -v cmake >/dev/null || ! command -v g++ >/dev/null; then
    apt-get update -qq
    apt-get install -y -qq build-essential ninja-build cmake git file ca-certificates wget patch
fi

# SDL3 builds from source via FetchContent; its Linux backends need these headers.
apt-get update -qq
apt-get install -y -qq \
    libx11-dev libxext-dev libxrandr-dev libxrender-dev libxfixes-dev libxi-dev \
    libxcursor-dev libxtst-dev libxss-dev libxinerama-dev \
    libxkbcommon-dev libwayland-dev libdecor-0-dev libdrm-dev libgbm-dev \
    libasound2-dev libpulse-dev libudev-dev libdbus-1-dev libibus-1.0-dev \
    libx11-xcb-dev libxcb1-dev libxcb-randr0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
    libxcb-icccm4-dev libxcb-keysyms1-dev libxcb-shm0-dev libxcb-cursor-dev \
    libegl1-mesa-dev libgles2-mesa-dev libgl1-mesa-dev libpipewire-0.3-dev \
    || true

cmake --version
g++ -dumpversion

git config --global --add safe.directory /src

cd /src

cmake --preset linux-x64-debug -DMAX_HEADLESS=ON -DCMAKE_INSTALL_PREFIX=/usr/local

cmake --build --preset linux-x64-debug -j"$(nproc)"

ls -la build/linux-x64-debug/max-port
