# `cargo_vectors` — C++-векторы экрана мобилизации (`CargoMenu`)

Генератор эталонных значений для P7: **не Rust**, а повтор формул и условий
C++-порта. Печатает списки, цены и стоимости апгрейдов, которые Rust-клиент
обязан воспроизвести бит-в-бит.

## Оракул (не меняется)

| Файл | Что взято |
|---|---|
| `max-cpp-port/src/cargo.cpp` | `Cargo_GetRawConsumptionRate` (`:140`) — ставка 1/2/3 |
| `max-cpp-port/src/cargomenu.cpp` | `CargoMenu::unit_types1` (`:176`), `BuyUnit` (`:546`) — `3 · ATTRIB_TURNS` |
| `max-cpp-port/src/purchasetypeselector.cpp` | цена строки списка (`:40`): `STATIONARY` → `CONSTRCT`, `COMMANDO`/`INFANTRY` → `TRAINHAL`, иначе `LANDPLT` |
| `max-cpp-port/src/abstractupgrademenu.cpp` | `IsBuildable` (`:681`), `IsUnitFiltered` (`:699`), `IsBetterUnit` (`:727`), `AddUpgradeMilitary/Generic/Mobile` (`:224/296/320`) |
| `max-cpp-port/src/upgradecontrol.cpp` | `Init` (шаг 1/2/5/10, `:102`), `CalculateCost` (`:58`), `UpgradeControl_CalculateCost` (`:214`, `std::pow`) |
| `assets/PATCHES.RES` | `SC_UNITS` (флаги, `Units::LoadResource`), `SC_ATTRI` (базовые атрибуты) |
| `default.game_rules.lua` | `max_builder_capability_list` — что какой строитель строит (та же таблица, что `max_core::production::BUILDABLE`) |

## Данные

`data.inc` — C++-фрагмент с 93 типами (флаги + turns/hits/armor/attack/speed/
range/rounds/scan/storage/ammo) и таблицей «строитель → юнит». Генерируется
из `assets/PATCHES.RES` (XOR-ключ `ResourceManager_GenericTable`):

```sh
python max-rust/tools/gen_cargo_data.py
```

## Запуск

```sh
docker run --rm -v "D:/dev/max-port-debug:/src" \
    -w /src/max-rust/tools/cargo_vectors max-port-build:latest \
    bash -c 'g++ -O2 -std=c++17 main.cpp -o /tmp/cargo_vectors && \
             /tmp/cargo_vectors > /src/max-rust/tools/cargo_vectors/vectors.txt'
```

`vectors.txt` закоммичен; `max_app/tests/cargo_vectors.rs` читает его напрямую.

## Формат

| Строка | Поля | Смысл |
|---|---|---|
| `purchase` | `index unit turns price` | `CargoMenu::unit_types1` и цена `BuyUnit` |
| `listprice` | `unit kind price` | цена строки списка (`kind` — `stationary`/`infantry`/`mobile`) |
| `upgrade_list` | `mask count unit…` | `PopulateTeamUnitsList`: маска фильтров (бит 0 ground, 1 air, 2 sea, 3 building, 4 combat), число и типы в порядке `IsBetterUnit` |
| `rows` | `unit count (attr cost)…` | строки апгрейдов типа (`attr` — `ATTRIB_*`: 0 attack, 1 rounds, 2 range, 3 armor, 4 hits, 5 speed, 6 scan, 8 ammo) и стоимость первого шага |

`unit` — индекс `UnitType` 0..92 (`resourcetable.hpp`).

## Результат

* список покупки — **15** типов;
* список модернизации по умолчанию — **62** типа (грунт+воздух+море+здания,
  без «только боевые»);
* строки апгрейдов — до 8 (attack/rounds/range/ammo/armor/hits/scan/speed),
  `ATTRIB_TURNS` не добавляется (`UPGRADE_CONTROL_9` в C++-порте не
  используется);
* Rust-сверка — 0 расхождений (`cargo test -p max_app --test cargo_vectors`).
