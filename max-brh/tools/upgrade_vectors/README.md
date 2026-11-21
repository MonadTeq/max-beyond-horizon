# `upgrade_vectors` — эталонные стоимости исследований/апгрейдов

Генератор векторов для P6 (`max-brh-docs/development/PLAN-production.md`). Повторяет
**float-формулы C++-порта** (`std::pow(..., 7.5)`) и печатает точные значения,
которые Rust-симуляция обязана воспроизвести **без `f64`** (DESIGN §6.4:
`#![deny(clippy::float_arithmetic)]`).

Оракул (не меняется):

| Файл | Что взято |
|---|---|
| `max-cpp-port/src/upgradecontrol.cpp` | `UpgradeControl_CalculateCost` (`upgradecontrol.cpp:214`), `UpgradeControl_Factors` (`:29`), шаг `UpgradeControl::Init` (`:113`) |
| `max-cpp-port/src/researchmenu.cpp` | `ResearchMenu_UpdateResearchProgress` (`:109`), `ResearchMenu_ApplyUpgrades` (`:137/196`), `ResearchMenu_CalculateFactor` (`:258`) |
| `max-cpp-port/src/teamunits.cpp` | `TeamUnits_UpgradeOffsetFactor` (`:271`), `TeamUnits_GetUpgradeCost` (`:189`) |
| `max-cpp-port/src/settings.cpp` | дефолты `*_factor` (`:70-77`: 16/16/8/32/32/16/8/8) |

## Запуск

```sh
docker run --rm -v "D:/dev/max-port-debug:/src" \
    -w /src/max-brh/tools/upgrade_vectors max-port-build:latest \
    bash -c 'g++ -O2 -std=c++17 main.cpp -o /tmp/upgrade_vectors && \
             /tmp/upgrade_vectors > /src/max-brh/tools/upgrade_vectors/vectors.txt'
```

`vectors.txt` закоммичен; копия для тестов —
`max-brh/crates/max_core/tests/data/upgrade_vectors.txt` (2.8 МБ, 135 452
строки). Тесты: `max-brh/crates/max_core/tests/research.rs`.

## Формат

| Строка | Поля | Смысл |
|---|---|---|
| `turns_default` | `topic level turns` | ходы исследования для дефолтного фактора темы |
| `research` | `factor level turns` | те же ходы для произвольного `factor` (1/2/4/8/16/32/64/128) |
| `factor` | `base_value level factor` | `ResearchMenu_CalculateFactor` = `base*level/10` |
| `apply` | `topic base_value level old new` | дельты `ResearchMenu_ApplyUpgrades` (для COST — формула `(level+9)/2`, кламп hits/speed к `u16`) |
| `offset` | `value offset` | `TeamUnits_UpgradeOffsetFactor` (1/2/5/10) |
| `cost` | `id current factor base cost` | `UpgradeControl_CalculateCost` |
| `cost_team` | `id base current factor step cost` | `TeamUnits_GetUpgradeCost`: разница уровней, минимум 1 |

`id` — `ATTRIB_*` (0 attack, 1 rounds, 2 range, 3 armor, 4 hits, 5 speed,
6 scan, 7 turns, 8 ammo). Для `cost`/`cost_team` генератор пропускает входы,
где C++ получает NaN/Inf (`current - factor <= 0` для не-turns);
переполнение `(int32_t)double` печатается как `-2147483648` — поведение x86
`cvttsd2si`, его же повторяет Rust (`i32::MIN`).

## Целочисленная формула (Rust)

`max_core::research` считает `(p/q)^7.5` в Q48:

```
(p/q)^7.5 = p^7 * sqrt(p*q) / q^8
v = 2^48; 7 × v = v*p/q;      // x^7 в Q48
root = isqrt(p * 2^96 / q);   // sqrt(x) в Q48
value = (v * root) / 2^48;    // x^7.5 в Q48
cost = value * factor / 2^48;
```

Результат сверяется с `std::pow`-векторами бит-в-бит: 58 086 стоимостей,
656 ходов исследования, 61 000+ дельт — 0 расхождений.
