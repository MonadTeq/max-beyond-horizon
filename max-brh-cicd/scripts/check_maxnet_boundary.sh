#!/usr/bin/env bash
# CI lock for Phase 5.3 (part A): the max_net → max_core dependency is a deliberate
# read-only projection of the simulation into the wire (owner decision, option 2).
#
# It verifies that `max-brh/crates/max_net/src` has no `max_core::` accesses
# outside the allowlist:
#
#   * five projections: `UnitState::from_unit`, `state_sync_for`,
#     `state_sync_tail`, `crc_barrier_for`, `complex_summary_for`;
#   * data imports: `use max_core::unit::UnitInfo`,
#     `use max_core::team::TeamUnits`.
#
# Note: the facade transport DTOs `NetworkCommand`/`RemoteCommand` are **not**
# allowlisted here, because they live in the lower layer (`max_types::universe`)
# and `wire/network.rs` imports them from there — `max_net` needs no
# `max_core::universe` at all.
#
# Everything else (especially `max_core::events::apply_*`, `GameState` mutations)
# voids the decision: see `max-brh-docs/development/PLAN-refactor.md` Phase 5.3 and
# `architect-rules.md`.
#
# How the file is read:
#   * comment lines (`//`, `///`, `//!`) are skipped — they are not calls;
#   * the contents of `#[cfg(test)] mod tests { ... }` (the file tail) are skipped:
#     tests do not ship in the production path;
#   * the current function is tracked, so multi-line signatures
#     (`pub fn state_sync_for(\n    state: &max_core::game::GameState, ...)`)
#     belong to their own function rather than to the previous one.
#
# Run: bash max-brh-cicd/scripts/check_maxnet_boundary.sh
# Exit code: 0 — the boundary holds, 1 — forbidden accesses found.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/max-brh/crates/max_net/src"

if [ ! -d "$SRC" ]; then
    echo "check_maxnet_boundary: нет каталога $SRC" >&2
    exit 2
fi

# awk scanner: prints violations and exits 1 if there are any.
scan() {
    awk '
        BEGIN {
            split("from_unit state_sync_for state_sync_tail crc_barrier_for complex_summary_for", names, " ")
            for (i in names) {
                allowed[names[i]] = 1
            }
            in_test = 0
            current = ""
            bad = 0
        }
        {
            line = $0

            # Tests in the file tail are not part of the max_net production path.
            if (in_test) {
                next
            }
            if (line ~ /^[ \t]*#\[cfg\(test\)\]/) {
                in_test = 1
                next
            }

            # Comments (including doc comments) are not calls.
            if (line ~ /^[ \t]*\/\//) {
                next
            }

            # End of a function/impl: drop the context at indentation 0..4.
            if (line ~ /^[ ]{0,4}\}[ ]*$/) {
                current = ""
            }

            # Start of a function: `fn name(` with any qualifiers.
            if (match(line, /(^|[ \t])(pub([ \t]*\([^)]*\))?[ \t]+)?(const[ \t]+)?(async[ \t]+)?(unsafe[ \t]+)?fn[ \t]+[A-Za-z0-9_]+/)) {
                name = line
                sub(/.*fn[ \t]+/, "", name)
                sub(/[^A-Za-z0-9_].*/, "", name)
                current = name
            }

            if (line !~ /max_core::/) {
                next
            }

            # Allowed imports of simulation data.
            if (line ~ /^[ \t]*(pub[ \t]+)?use[ \t]+max_core::(unit::UnitInfo|team::TeamUnits)([;{ \t]|$)/) {
                next
            }

            # Allowed five read-only projections.
            if (current in allowed) {
                next
            }

            printf "%s:%d: %s\n", FILENAME, FNR, line
            bad++
        }
        END {
            exit (bad > 0)
        }
    ' "$1"
}

status=0
checked=0

while IFS= read -r file; do
    checked=$((checked + 1))
    if ! scan "$file"; then
        status=1
    fi
done < <(find "$SRC" -type f -name '*.rs' | sort)

if [ "$status" -ne 0 ]; then
    cat >&2 <<'EOF'

check_maxnet_boundary: НАРУШЕНИЕ границы max_net → max_core.

Зависимость max_net → max_core разрешена только как read-only проекция
симуляции в провод (решение владельца, вариант 2, Ф5.3):

  UnitState::from_unit, state_sync_for, state_sync_tail,
  crc_barrier_for, complex_summary_for
  + use max_core::unit::UnitInfo / max_core::team::TeamUnits.

Новые вызовы max_core, вызовы apply_* и мутации GameState из max_net
запрещены без пересмотра решения
(max-brh-docs/development/PLAN-refactor.md Ф5.3, architect-rules.md).
EOF
    exit 1
fi

printf 'check_maxnet_boundary: OK — %d файлов max_net/src, граница соблюдена.\n' "$checked"
