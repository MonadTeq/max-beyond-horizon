# Build, package and gate image for the portable Linux distribution.
#
# Plan: max-brh-docs/development/distro-installers.md §D/§F, stage 4.
# Pattern: release.sh builds the Linux client in a throwaway container with the
# same Rust toolchain as the Windows release; this image only adds the packaging
# and gate tooling (squashfs-tools/appimagetool, Xvfb for the GUI smoke).
#
# Ubuntu 22.04 is the runtime baseline: the client and the SDL3 build link
# against its glibc (2.35), so the tar.gz and the AppImage run on 22.04 and
# newer without FUSE/build dependencies on the player's machine.
FROM ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Build dependencies of sdl-from-source,sound (the release.sh list) plus the
# packaging tools: jq reads data-inventory.json, patchelf writes the $ORIGIN
# rpath, ImageMagick makes the desktop icon, squashfs-tools feeds appimagetool.
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
        build-essential cmake pkg-config git ca-certificates curl file \
        xz-utils jq patchelf imagemagick \
        libasound2-dev libx11-dev libxext-dev libxrender-dev libxcursor-dev \
        libxi-dev libxrandr-dev libxinerama-dev libxss-dev libxtst-dev \
        libxfixes-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
        wayland-protocols libdecor-0-dev libegl-dev libgles-dev libgl-dev \
        libgbm-dev libdrm-dev libdbus-1-dev libudev-dev libpipewire-0.3-dev \
        libpulse-dev \
        libgl1 libglx-mesa0 libgl1-mesa-dri \
        squashfs-tools zsync \
        xvfb xauth x11-utils xdotool \
    && rm -rf /var/lib/apt/lists/*

# Rust 1.94.0 — the same compiler release.sh uses for the Linux binary.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain 1.94.0 \
    && rustc --version \
    && cargo --version

# appimagetool: a pinned versioned release, not `continuous`, so that a rebuild
# packages with the same tool. The tool is itself an AppImage: it is extracted
# here (no FUSE in containers) and exposed as `appimagetool`.
ARG APPIMAGETOOL_VERSION=1.9.1
ARG APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUN curl -fsSL -o /opt/appimagetool.AppImage \
        "https://github.com/AppImage/appimagetool/releases/download/${APPIMAGETOOL_VERSION}/appimagetool-x86_64.AppImage" \
    && echo "${APPIMAGETOOL_SHA256}  /opt/appimagetool.AppImage" | sha256sum -c - \
    && chmod +x /opt/appimagetool.AppImage \
    && cd /opt \
    && APPIMAGE_EXTRACT_AND_RUN=1 ./appimagetool.AppImage --appimage-extract >/dev/null \
    && ln -sf /opt/squashfs-root/AppRun /usr/local/bin/appimagetool

WORKDIR /src
