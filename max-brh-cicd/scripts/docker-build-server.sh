#!/bin/bash
# Builds the headless dedicated server (MAX_HEADLESS) with the CI cross toolchain.
# Invoked as: docker run --rm -v "D:/dev/max-port-debug:/src" -w /src max-port-build:latest bash /src/build/docker-build-server.sh
set -euxo pipefail

export DEBIAN_FRONTEND=noninteractive

# The max-port-build image already carries the toolchain; this branch exists
# only for running the script inside a bare ubuntu:24.04 container.
if ! command -v cmake >/dev/null || ! command -v x86_64-w64-mingw32-g++ >/dev/null; then
    apt-get update -qq
    apt-get install -y -qq \
        build-essential ninja-build cmake git file ca-certificates wget patch \
        binutils-mingw-w64-x86-64 gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64
fi

cmake --version
x86_64-w64-mingw32-g++ -dumpversion

git config --global --add safe.directory /src

cd /src

cmake -S . -B build/cross-winxp-x64-server-debug -G Ninja \
    -DCMAKE_BUILD_TYPE=Debug \
    -DCMAKE_TOOLCHAIN_FILE=cmake/toolchain-cross-mingw-w64-x86_64.cmake \
    -DCMAKE_INSTALL_PREFIX=/usr/local \
    -DMAX_HEADLESS=ON

cmake --build build/cross-winxp-x64-server-debug -j"$(nproc)"

ls -la build/cross-winxp-x64-server-debug/*.exe
