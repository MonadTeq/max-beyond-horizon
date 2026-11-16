#!/usr/bin/env bash
# Прогон всех QA-сценариев + golden (для docker: rust:1.94.0-bookworm).
set -u
cd /src/max-rust
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-target}
for s in sp-full hs-2 visual-screens victory production save-load upgrade research edge-cases cargo-full move-input; do
  cargo run -q --no-default-features -p max_app --example qa_run -- \
    "/src/max-rust-qa/scripts/$s.qa" "/src/max-rust-qa/out/$s" >/dev/null 2>&1 \
    || { echo "FAIL $s"; exit 1; }
done
echo "QA-ALL-DONE"
cargo run -q --no-default-features -p max_app --example qa_golden_check -- \
  /src/max-rust-qa/golden /src/max-rust-qa/out/visual-screens 2>&1 | tail -2
