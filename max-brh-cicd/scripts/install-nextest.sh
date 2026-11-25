#!/usr/bin/env bash
# Install cargo-nextest for the CI container (test-organization.md, Ar3/Q4: the
# standard test runner is nextest, not `cargo test`).
#
# A nextest already on PATH is reused. Otherwise the official prebuilt binary is
# downloaded (no compilation, a few seconds); when the download is unavailable
# the script falls back to `cargo install cargo-nextest --locked`.
#
# Environment: NEXTEST_VERSION (default: latest), NEXTEST_BIN_DIR (default:
# /usr/local/bin; must be writable — the Docker stages run as root).
set -euo pipefail

if cargo nextest --version >/dev/null 2>&1; then
    echo "nextest: $(cargo nextest --version) (already installed)"
    exit 0
fi

NEXTEST_VERSION="${NEXTEST_VERSION:-latest}"
BIN_DIR="${NEXTEST_BIN_DIR:-/usr/local/bin}"

if command -v curl >/dev/null 2>&1 &&
    curl -fsSL "https://get.nexte.st/${NEXTEST_VERSION}/linux" | tar zxf - -C "$BIN_DIR" &&
    cargo nextest --version >/dev/null 2>&1; then
    echo "nextest: $(cargo nextest --version) (prebuilt binary from get.nexte.st)"
    exit 0
fi

echo "nextest: prebuilt download unavailable, falling back to cargo install" >&2
cargo install cargo-nextest --locked
echo "nextest: $(cargo nextest --version) (cargo install)"
