#!/bin/bash
# Builds the Windows x64 executable with the CI cross toolchain inside Docker.
# Invoked as: docker run --rm -v "D:/dev/max-port-debug:/src" -w /src ubuntu:24.04 bash /src/build/docker-build.sh
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

# The cross presets inherit CMAKE_INSTALL_PREFIX "c:/Program Files/M.A.X. Port", which
# configure_package_config_file (SDL3, json-schema-validator) rejects under CMake 3.28
# on a Linux host. The install prefix is irrelevant for a build-tree run, so override it.
cmake --preset cross-windowsxp-x64-debug -DCMAKE_INSTALL_PREFIX=/usr/local
cmake --build --preset cross-windowsxp-x64-debug -j"$(nproc)"

ls -la build/cross-winxp-x64-debug/*.exe
