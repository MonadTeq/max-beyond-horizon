# Игровые механики M.A.X. (выверено по C++-порту)

Сводка правил, которые мы портировали и проверяли по оракулу `max-cpp-port`
(источник — построчный разбор функций, ссылки ниже). Этот файл — «контракт»:
перед любым изменением экономики/производства/боя сверяйтесь с ним, чтобы
починкой одного не сломать другое.

Обозначения: `raw` — материалы, `fuel` — топливо, `gold` — золото, `power` —
энергия, `life/workers` — рабочие.

---

## 1. Ресурсы и их расход

- `Cargo_ApplyUnitConsumption(unit_type, speed_multiplier, cargo)` (`cargo.cpp`):
  `raw -= Cargo_GetRawConsumptionRate(unit_type, speed_multiplier)`,
  `fuel -= Cargo_GetFuelConsumptionRate(unit_type)`,
  `power -= Cargo_GetPowerConsumptionRate(unit_type)`,
  `life -= Cargo_GetLifeConsumptionRate(unit_type)`,
  `gold -= Cargo_GetGoldConsumptionRate(unit_type)`.
- `Cargo_GetRawConsumptionRate(unit_type, multiplier)`: multiplier 1→1, 2→4, 4→12.
- `Cargo_GetGoldConsumptionRate`: `COMMTWR = 5`, всё остальное **0** — т.е.
  производство заводов золото **не** тратит (только `COMMTWR`).
- `Cargo_GetFuelConsumptionRate`: `POWERSTN = 6`, `POWGEN = 2`, остальные 0.
- `Cargo_GetPowerConsumptionRate`: заводы/SHIPYARD/LANDPLT/AIRPLT/LIGHTPLT/
  TRAINHAL/GREENHSE/RESEARCH/COMMTWR/MININGST = 1; `POWERSTN` — производитель
  (отрицательное потребление); `POWGEN` — производитель.
- `Cargo_GetLifeConsumptionRate`: `POWERSTN`/`POWGEN`/`HABITAT` — рабочие.

## 2. Комплекс, коннекторы, ёмкости

- Комплекс = связный граф зданий, соединённых **коннекторами**
  (`UnitsManager_UpdateConnectors`, `units_manager.cpp:2051-2225`); слияние —
  `AttachToPrimaryComplex`/`AttachComplex` (по наименьшему id), распад —
  `TestConnections`/`DetachComplex`.
- `IsComplexMember`: `(CONNECTOR_UNIT|BUILDING|STANDALONE) && !GROUND_COVER &&
  id != NO_ID`; достроенное, но не раскрытое здание (`ORDER_IDLE/BUILDING_READY`)
  в комплекс **не входит** до `BuildingReady`.
- Ёмкость/запас комплекса — сумма по членам: `Complex::GetCargoInfo` =
  `Cargo_GetInventory` (`storage`) + `Cargo_GetCargoCapacity` (`ATTRIB_STORAGE`
  по типу груза). Каждое хранилище имеет свой объём: `ADUMP`(raw)/`FDUMP`(fuel)/
  `GOLDSM`(gold) = 50; `MININGST` = 25; `CONSTRCT` = 60; `SPLYTRCK`/`FUELTRCK`/
  `GOLDTRCK` = 50.
- `Complex::Transfer(delta)` меняет кэш и раздаёт разницу по складам в порядке
  `UnitsManager_StationaryUnits` (`TransferCargo`).
- Каждый ход `Access_UpdateResourcesTotal` пересчитывает кэш как
  `Cargo_GetNetProduction + Cargo_GetInventory`, а `UnitInfo::UpdateProduction`
  раздаёт его по складам. x-fer берёт из **общего котла** комплекса.

## 3. Производство

- **Мобильный строитель** (конструктор/инженер, `MOBILE_LAND_UNIT`): платит
  `Cargo_GetRawConsumptionRate(unit_type, build_rate)` из **своего `storage`**
  (`UnitInfo::UpdateProduction`, `unitinfo.cpp:2904`); при нехватке — пауза,
  `build_time` не двигается, в минус не уходит.
- **Завод** (SHIPYARD/LIGHTPLT/LANDPLT/TRAINHAL/AIRPLT): при `ORDER_BUILD &&
  state != UNIT_READY` — `Cargo_ApplyUnitConsumption(unit_type, max_build_rate)`
  из **общего котла комплекса** (raw + power + life; `fuel` — только через
  производителей энергии, `gold` не тратит). **Старт** требует, чтобы первый ход
  был оплачен (raw+power+life, п.5): иначе заказ не отдаётся
  (`NotEnoughPower/Workers/Materials`), очередь не теряется. Нехватка в ходе
  стройки → пауза (`PausedNoMaterials`/`PausedNoPower`/…), `build_time` не
  двигается, очередь сохраняется.
- Завершение: `build_time <= build_rate` → `SpawnNewUnit`; ставка режется по
  складу (`BuildMenu_GetMaxPossibleBuildRate`).
- **Построенный юнит** (завод): встаёт на клетку завода, `ORDER_IDLE/STORE`,
  `parent`=завод, `scaler_adjust=4`; выводится игроком (`ORDER_ACTIVATE` →
  `ActivateUnit`: `--storage`, `SetParent(nullptr)`, самолёту `ORDER_MOVE/INIT`
  + `ScaleUnit(EXPAND)`, наземному — клетка клика). `repeat_build` продолжает.
- **Стройка здания** (мобильный строитель): площадка (`LRGTAPE`+`LRGCONES`+
  `LRGSLAB` у зданий, иначе малые; вода — без конусов, угол 1); строитель
  въезжает в центр (`PositionInTape`); при завершении — `UNIT_READY`, маркеры
  `BLDMRK1..5` по кольцу; **здание раскрывается после ухода строителя**
  (`BuildingReady`), плита остаётся. **Увести строителя во время стройки
  нельзя** (`CURSOR_UNIT_NO_GO`) — отмена только явным `stop` (`CancelBuilding`).

## 4. Шахта и добыча

- `MININGST` при `ORDER_POWER_ON`/`NEW_ALLOCATE`: `cargo.gold += gold_mining ×
  difficulty/4`, `raw`, `fuel` аналогично; `difficulty_factor` = 4 (человек),
  5 (компьютер `MASTER`), 6 (`GOD`); `free_capacity = 16 − total_mining`.
- `raw_mining + fuel_mining + gold_mining ≤ 16`; распределение — диалог
  `Allocate` (ALLOCFRM).

## 5. Энергия и рабочие

- Потребление/производство — ставки п.1 (`cargo.cpp:203-233`): `POWERSTN` +6,
  `POWGEN` +1, а `SHIPYARD`/`LIGHT/LAND/AIRPLT`/`TRAINHAL`/`GREENHSE`/`RESEARCH`/
  `COMMTWR`/`MININGST` — по 1. Баланс комплекса — `Σ Cargo_GetNetProduction`
  (`cargo.cpp:70-130`) + инвентарь (`Access_UpdateResourcesTotal`,
  `access.cpp:912-934`).
- **Запуск производства** (`UnitInfo::StartBuilding`, `unitinfo.cpp:2751-2794`)
  сразу списывает из котла `complex->material`, `complex->power`,
  `complex->workers` и зовёт `GameManager_OptimizeProduction`
  (`game_manager.cpp:2555-2584`). Если дефицит не закрывается (энергия, топливо,
  рабочие, материалы), `ValidateIndustry` (`production_manager.cpp:677-711`)
  не даёт заказу стартовать (`ORDER_STATE_BUILD_ABORT`); в порте —
  `BuildError::NotEnoughPower/Fuel/Workers/Materials`, очередь сохраняется.
- **Каждый ход** `TeamUnits::OptimizeComplexes` (`teamunits.cpp:170-175`) →
  `ProductionManager_OptimizeProduction` (`production_manager.cpp:830-901`)
  включает/выключает производителей и гасит потребителей. Порядок отключения при
  дефиците энергии: `RESEARCH` → `GREENHSE` → `COMMTWR` → стройки
  (`ValidateIndustry`) → `MININGST` (`OptimizeMiningIndustry`).
- Генераторы включаются автоматически при спросе и запасе топлива
  (`OptimizePowerProduction`/`OptimizePowerConsumption`, `production_manager.cpp:
  481-562`, `CheckPowerNeed` `:642-664`); без топлива (`POWGEN` 2, `POWERSTN` 6
  за ход) производитель не работает. Выключенные из-за дефицита потребители C++
  сам обратно **не** включает — это делает игрок (`power on`) или ИИ.
- `HABITAT` (`life = -3`) при нехватке рабочих включается
  `UpdateUnitLifeConsumption` (`production_manager.cpp:366-380`); `TRAINHAL`/
  `GREENHSE`/`RESEARCH` тратят по 1 рабочему.
- **Сообщение над картой** — `ComposeIndustryMessage`/`DrawResourceMessage`
  (`production_manager.cpp:308-330/465-479/792-804`), `MESSAGE_BOX_NOTICE`:
  «Adjustments made:», «Cannot turn X on, power needed.», «More power needed,
  X turned off./on.». Материал выбирается по первому дефициту: `raw material`,
  `fuel`, `gold`, `power`, `workers`.

## 6. Бой

- Снаряд по типу оружия (`ROCKET`/`TORPEDO`/`ALNTBALL`/`ALNABALL`/`ALNMISSL`),
  летит по прямой попиксельно (`AirPath::Execute`), хвост `Paths_DrawMissile`
  (`RKTSMOKE`/`TRPBUBLE`, 3 частицы на клетку).
- Попадание → взрыв по цели (`HITEXPLD`/`BLDEXPLD`/`LND/AIR/SEAEXPLD`), промах —
  `HITEXPLD`; взрыв листает кадры (`frames`, кадр за 2 тика), на половине
  анимации отпускает `parent` и уничтожает погибшего; наземный юнит оставляет
  обломок (`SMLRUBLE`/`LRGRUBLE`, `UnitsManager_BuildClearing`), расчищается
  бульдозером.
- Выстрел: `PrepareFire` (`firing_recoil_frames`, вспышка `firing_image_base`),
  звук `SFX_TYPE_FIRE`; взрыв — `GEN_XPLD`.

## 7. Движение

- Угол по ходу: `Paths_UpdateAngle` (поворот 45° за тик, движение только «лицом»),
  башня поворачивается вместе с корпусом.

---

Ссылки: все функции — в `max-cpp-port/src/{cargo,complex,production_manager,
units_manager,unitinfo,access,game_manager,airpath,paths}.cpp`; детали порта и
тестов — `max-brh-docs/analysis/visual-1to1-status.md` (§39–§41), `max-brh-docs/architecture/
state-model.md`.
