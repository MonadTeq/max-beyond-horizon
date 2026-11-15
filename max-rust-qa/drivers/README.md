# Драйверы и утилиты QA

## Драйверы (исполняемые примеры в `max-rust/crates/max_app/examples/`)

| Драйвер | Назначение | Запуск |
|---|---|---|
| `qa_run.rs` | исполнение .qa-скриптов через реальные обработчики ввода (без SDL) | `cargo run -p max_app --example qa_run -- <script.qa> <out_dir>` |
| `qa_golden_check.rs` | сверка кадров с эталонами, `--update` — перегенерация | `cargo run -p max_app --example qa_golden_check -- [--update] <golden_dir> <out_dir>` |
| `sp_frames.rs` | генерация эталонных кадров экранов | `cargo run -p max_app --example sp_frames -- <data_dir> <out_dir>` |

## Оконные прогоны (Windows, реальный бинарь + мышь)

| Драйвер | Назначение |
|---|---|
| `build/scripts/ui-test-sp.ps1` | клики мышью через user32: меню→настройка→партия→скриншоты |
| `build/scripts/live-smoke.sh` | сетевой smoke: Horizon + headless + 2 клиента × N ходов |

## Утилиты

| Утилита | Назначение |
|---|---|
| `max-rust/tools/rng_vectors/` | C++-генератор эталонных векторов RNG |
| `max-rust/tools/map_vectors/` | C++-генератор эталонов генерации карты |
| `max-rust/tools/sim_vectors/` | C++ мини-реплей для паритета дайджеста |
| `max-rust/tools/lobby_vectors/`, `barrier_vectors/`, `font_vectors/` | байтовые эталоны пакетов/шрифтов |
| `max-original-dasm/tools/*_parser.py` | эталонные Python-парсеры форматов |

## Формат .qa-скрипта (кратко)

```
# комментарий
menu new_game|multiplayer|hot_seat|join
setup done|cancel|random
players <slot> human|computer|none
select X Y | select unit N
move X Y | move grid X Y | move near
end_turn
click X Y | rclick X Y
key ENTER|ESC|T|...
type "text"
wait N          # до N кадров до готовности (или мс?)
tick N
shot <name>     # PNG кадра
assert turn>=3 | assert team=0 | assert units>=14 | assert screen=InGame | assert victory | assert defeat
save <name>     # MXLS
load <file>     # V70 .CAM
exit
```

Полный формат и примеры — в `plans/qa-plan.md` и скриптах `../scripts/`.
