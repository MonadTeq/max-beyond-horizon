#!/usr/bin/env bash
# CI-замок Ф5.3 (часть A): зависимость max_net → max_core — осознанная
# read-only проекция симуляции в провод (решение владельца, вариант 2).
#
# Проверяется, что в `max-brh/crates/max_net/src` нет обращений к `max_core::`
# вне белого списка:
#
#   * пять проекций: `UnitState::from_unit`, `state_sync_for`,
#     `state_sync_tail`, `crc_barrier_for`, `complex_summary_for`;
#   * импорты данных: `use max_core::unit::UnitInfo`,
#     `use max_core::team::TeamUnits`.
#
# Всё остальное (особенно `max_core::events::apply_*`, мутации `GameState`)
# аннулирует решение: см. `max-brh-docs/development/PLAN-refactor.md` Ф5.3 и
# `architect-rules.md`.
#
# Как читается файл:
#   * строки-комментарии (`//`, `///`, `//!`) пропускаются — они не вызовы;
#   * содержимое `#[cfg(test)] mod tests { ... }` (хвост файла) пропускается:
#     тесты не поставляются в прод-путь;
#   * отслеживается текущая функция, поэтому многострочные сигнатуры
#     (`pub fn state_sync_for(\n    state: &max_core::game::GameState, ...)`)
#     относятся к своей функции, а не к предыдущей.
#
# Запуск: bash max-brh-cicd/scripts/check_maxnet_boundary.sh
# Код возврата: 0 — граница соблюдена, 1 — есть запрещённые обращения.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)"
SRC="$ROOT/max-brh/crates/max_net/src"

if [ ! -d "$SRC" ]; then
    echo "check_maxnet_boundary: нет каталога $SRC" >&2
    exit 2
fi

# awk-сканер: печатает нарушения и завершается 1, если они есть.
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

            # Тесты в хвосте файла не входят в прод-путь max_net.
            if (in_test) {
                next
            }
            if (line ~ /^[ \t]*#\[cfg\(test\)\]/) {
                in_test = 1
                next
            }

            # Комментарии (в т.ч. doc-комментарии) — не вызовы.
            if (line ~ /^[ \t]*\/\//) {
                next
            }

            # Конец функции/impl: сбрасываем контекст на отступе 0..4.
            if (line ~ /^[ ]{0,4}\}[ ]*$/) {
                current = ""
            }

            # Начало функции: `fn name(` с любыми квалификаторами.
            if (match(line, /(^|[ \t])(pub([ \t]*\([^)]*\))?[ \t]+)?(const[ \t]+)?(async[ \t]+)?(unsafe[ \t]+)?fn[ \t]+[A-Za-z0-9_]+/)) {
                name = line
                sub(/.*fn[ \t]+/, "", name)
                sub(/[^A-Za-z0-9_].*/, "", name)
                current = name
            }

            if (line !~ /max_core::/) {
                next
            }

            # Разрешённые импорты данных симуляции.
            if (line ~ /^[ \t]*(pub[ \t]+)?use[ \t]+max_core::(unit::UnitInfo|team::TeamUnits)([;{ \t]|$)/) {
                next
            }

            # Разрешённые five read-only projections.
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
