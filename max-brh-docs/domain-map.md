# Domain Map — соответствие доменного языка и оракулов

Каноническая таблица соответствия «домен-термин → Rust-имя → C++-идентификатор
→ источник → примечание». Единый язык домена (DSL) для аналитиков, дизайнеров,
программистов и архитекторов. Источники: Оракул Interplay (оригинал, приоритет
истины) и Оракул Klei (C++-порт, имена/структуры). Пополняется при введении
каждой новой сущности
(правило 6 в [`development/rust-programming-rules.md`](development/rust-programming-rules.md)).

Формат строки:

| Домен-термин | Rust-имя | C++-идентификатор | Источник (файл:строка) | Примечание |
|---|---|---|---|---|

## Юниты

Каноничные DSL-имена (источник — мануал M.A.X. + оракулы (Interplay/Klei) + решения владельца).
В столбце «Примечание» — где текущее Rust-имя `UnitType` требует переименования.

| Домен-термин | Rust-имя | C++ | Примечание |
|---|---|---|---|
| Очиститель золота | `GoldRefinery` | `COMMTWR` | переименовать `CommTower` |
| Электростанция | `PowerStation` | `POWERSTN` | — |
| Энергогенератор | `PowerGenerator` | `POWGEN` | — |
| Казарма | `Barracks` | `BARRACKS` | — |
| Щитовой генератор | `ShieldGenerator` | `SHIELDGN` | — |
| Радар | `Radar` | `RADAR` | — |
| Склад материалов | `MaterialStorage` | `ADUMP` | переименовать `ADump` |
| Склад топлива | `FuelStorage` | `FDUMP` | переименовать `FDump` |
| Золотое хранилище | `GoldVault` | `GOLDSM` | переименовать `GoldSmelter` |
| Депо (ремонт/апгрейд) | `Depot` | `DEPOT` | — |
| Ангар | `Hangar` | `HANGAR` | — |
| Док | `Dock` | `DOCK` | — |
| Коннектор | `Connector` | `CNCT_4W` | — |
| Дорога | `Road` | `ROAD` | — |
| Посадочная площадка | `LandingPad` | `LANDPAD` | — |
| Верфь | `Shipyard` | `SHIPYARD` | — |
| Завод лёгкой техники | `LightVehiclePlant` | `LIGHTPLT` | — |
| Завод тяжёлой техники | `HeavyVehiclePlant` | `LANDPLT` | — |
| Завод поддержки | `SupportPlant` | `SUPRTPLT` | — |
| Авиазавод | `AirUnitsPlant` | `AIRPLT` | — |
| Жилище | `Habitat` | `HABITAT` | — |
| Исследовательский центр | `ResearchCentre` | `RESEARCH` | переименовать `ResearchCenter` |
| Экосфера | `EcoSphere` | `GREENHSE` | переименовать `Greenhouse` |
| Центр отдыха | `RecreationCenter` | `RECCENTR` | — |
| Тренировочный зал | `TrainingHall` | `TRAINHAL` | — |
| Водная платформа | `WaterPlatform` | `WTRPLTFM` | — |
| Орудийная башня | `GunTurret` | `GUNTURRT` | — |
| Зенитка | `AntiAir` | `ANTIAIR` | — |
| Артиллерийская башня | `Artillery` | `ARTYTRRT` | переименовать `ArtilleryTurret` |
| Противоракетная установка | `MissileLauncher` | `ANTIMSSL` | переименовать `AntiMissile` |
| Бетонный блок | `Block` | `BLOCK` | — |
| Мост | `Bridge` | `BRIDGE` | — |
| Шахта | `MiningStation` | `MININGST` | — |
| Наземная мина | `LandMine` | `LANDMINE` | — |
| Морская мина | `SeaMine` | `SEAMINE` | — |
| Конструктор | `Constructor` | `CONSTRCT` | — |
| Разведчик (амфибийный) | `Scout` | `SCOUT` | `MOBILE_SEA_UNIT\|MOBILE_LAND_UNIT` |
| Танк | `Tank` | `TANK` | — |
| Штурмовое орудие | `AssaultGun` | `ARTILLRY` | — |
| Ракетная установка | `RocketLauncher` | `ROCKTLCH` | — |
| Ракетный транспортёр | `MissileCrawler` | `MISSLLCH` | — |
| Мобильная ПВО | `MobileAntiAir` | `SP_FLAK` | — |
| Миноукладчик | `MineLayer` | `MINELAYR` | — |
| Сюрвейер | `Surveyor` | `SURVEYOR` | — |
| Мобильный сканер | `Scanner` | `SCANNER` | — |
| Грузовик снабжения | `SupplyTruck` | `SPLYTRCK` | — |
| Золотой грузовик | `GoldTruck` | `GOLDTRCK` | — |
| Инженер | `Engineer` | `ENGINEER` | — |
| Бульдозер | `Bulldozer` | `BULLDOZR` | — |
| Ремонтный юнит | `RepairUnit` | `REPAIR` | — |
| Топливный грузовик | `FuelTruck` | `FUELTRCK` | — |
| Бронетранспортёр | `ArmouredPersonnelCarrier` | `CLNTRANS` | переименовать `PersonnelCarrier` |
| Диверсант | `Infiltrator` | `COMMANDO` | переименовать `Commando` |
| Пехота | `Infantry` | `INFANTRY` | — |
| Эскорт | `Escort` | `FASTBOAT` | переименовать `FastBoat` |
| Корвет | `Corvette` | `CORVETTE` | — |
| Канонерка | `Gunboat` | `BATTLSHP` | переименовать `Battleship` |
| Подлодка | `Submarine` | `SUBMARNE` | — |
| Морской транспорт | `SeaTransport` | `SEATRANS` | — |
| Ракетный крейсер | `MissileCruiser` | `MSSLBOAT` | переименовать `MissileBoat` |
| Морской миноукладчик | `SeaMineLayer` | `SEAMNLYR` | — |
| Грузовое судно | `CargoShip` | `CARGOSHP` | — |
| Истребитель | `Fighter` | `FIGHTER` | — |
| Бомбардировщик | `Bomber` | `BOMBER` | — |
| Воздушный транспорт | `AirTransport` | `AIRTRANS` | — |
| AWAC | `Awac` | `AWAC` | — |
| Джаггернаут (alien) | `Juggernaut` | `JUGGRNT` | — |
| Танк пришельцев | `AlienTank` | `ALNTANK` | — |
| Штурмовое орудие пришельцев | `AlienAssaultGun` | `ALNASGUN` | — |
| Самолёт пришельцев | `AlienPlane` | `ALNPLANE` | — |

Не-юниты (эффекты/декорации) — имена Rust сохраняются; отдельная таблица в
`object-classifier.md`.

## Приказы

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
| Ожидание | `UnitOrder::Await` | `ORDER_AWAIT` | C++ | 0x00; дефолтное состояние |
| Трансформация | `UnitOrder::Transform` | `ORDER_TRANSFORM` | C++ | 0x01; смена формы/масштаба (развёртывание шахты) |
| Движение | `UnitOrder::Move` | `ORDER_MOVE` | C++ | 0x02 |
| Огонь | `UnitOrder::Fire` | `ORDER_FIRE` | C++ | 0x03 |
| Строительство | `UnitOrder::Build` | `ORDER_BUILD` | C++ | 0x04; конструктор/инженер/завод |
| Активация | `UnitOrder::Activate` | `ORDER_ACTIVATE` | C++/мануал | 0x05; извлечь из депо/ангара/дока, включить здание |
| Перераспределение добычи | `UnitOrder::NewAllocate` | `ORDER_NEW_ALLOCATE` | C++/мануал | 0x06; аллокация шахты (кнопка «Allocate») |
| Включение энергии | `UnitOrder::PowerOn` | `ORDER_POWER_ON` | C++ | 0x07 |
| Отключение энергии | `UnitOrder::PowerOff` | `ORDER_POWER_OFF` | C++ | 0x08 |
| Подрыв | `UnitOrder::Explode` | `ORDER_EXPLODE` | C++ | 0x09 |
| Выгрузка | `UnitOrder::Unload` | `ORDER_UNLOAD` | C++ | 0x0A |
| Расчистка | `UnitOrder::Clear` | `ORDER_CLEAR` | C++ | 0x0B; бульдозер |
| Дозор | `UnitOrder::Sentry` | `ORDER_SENTRY` | C++ | 0x0C |
| Посадка | `UnitOrder::Land` | `ORDER_LAND` | C++ | 0x0D |
| Взлёт | `UnitOrder::TakeOff` | `ORDER_TAKE_OFF` | C++ | 0x0E |
| Погрузка | `UnitOrder::Load` | `ORDER_LOAD` | C++ | 0x0F |
| Простой | `UnitOrder::Idle` | `ORDER_IDLE` | C++ | 0x10; отключён |
| Ремонт | `UnitOrder::Repair` | `ORDER_REPAIR` | C++ | 0x11 |
| Дозаправка | `UnitOrder::Refuel` | `ORDER_17` | C++/V70 | 0x12; **не реализован в C++-порте** (V70 `ORDER_TYPE_REFUEL`); зарезервирован под будущее расширение |
| Перезарядка | `UnitOrder::Reload` | `ORDER_RELOAD` | C++ | 0x13 |
| Передача | `UnitOrder::Transfer` | `ORDER_TRANSFER` | C++/мануал | 0x14; кнопка «Transfer» |
| Пауза строительства | `UnitOrder::HaltBuilding` | `ORDER_HALT_BUILDING` | C++ | 0x15 |
| Ожидание масштабирования | `UnitOrder::AwaitScaling` | `ORDER_AWAIT_SCALING` | C++ | 0x16 |
| Ожидание позиционирования ленты | `UnitOrder::AwaitTapePositioning` | `ORDER_AWAIT_TAPE_POSITIONING` | C++ | 0x17 |
| Ожидание захвата юнита | `UnitOrder::AwaitStealUnit` | `ORDER_AWAIT_STEAL_UNIT` | C++ | 0x18; диверсант |
| Ожидание вывода из строя | `UnitOrder::AwaitDisableUnit` | `ORDER_AWAIT_DISABLE_UNIT` | C++ | 0x19; диверсант |
| Вывод из строя | `UnitOrder::Disable` | `ORDER_DISABLE` | C++ | 0x1A |
| Движение к юниту | `UnitOrder::MoveToUnit` | `ORDER_MOVE_TO_UNIT` | C++ | 0x1B |
| Модернизация | `UnitOrder::Upgrade` | `ORDER_UPGRADE` | C++ | 0x1C |
| Установка мин | `UnitOrder::LayMine` | `ORDER_LAY_MINE` | C++ | 0x1D |
| Движение в атаку | `UnitOrder::MoveToAttack` | `ORDER_MOVE_TO_ATTACK` | C++ | 0x1E |
| Пауза строительства (2) | `UnitOrder::HaltBuilding2` | `ORDER_HALT_BUILDING_2` | C++ | 0x1F; авто-возобновление при ресурсах |

## Состояния

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
| Инициализация | `UnitOrderState::Init` | `ORDER_STATE_INIT` | C++ | 0x00 |
| Исполнение | `UnitOrderState::ExecutingOrder` | `ORDER_STATE_EXECUTING_ORDER` | C++ | 0x01 |
| Готов к исполнению | `UnitOrderState::ReadyToExecuteOrder` | `ORDER_STATE_READY_TO_EXECUTE_ORDER` | C++ | 0x02 |
| Хранение | `UnitOrderState::Store` | `ORDER_STATE_STORE` | C++ | 0x03 |
| Подготовка к хранению | `UnitOrderState::PrepareStore` | `ORDER_STATE_PREPARE_STORE` | C++ | 0x04 |
| В процессе | `UnitOrderState::InProgress` | `ORDER_STATE_IN_PROGRESS` | C++ | 0x05 |
| В транзите | `UnitOrderState::InTransition` | `ORDER_STATE_IN_TRANSITION` | C++ | 0x06 |
| Поиск пути | `UnitOrderState::IssuingPath` | `ORDER_STATE_ISSUING_PATH` | C++ | 0x07 |
| Готов к огню | `UnitOrderState::ReadyToFire` | `ORDER_STATE_READY_TO_FIRE` | C++ | 0x08 |
| Огонь | `UnitOrderState::FireInProgress` | `ORDER_STATE_FIRE_IN_PROGRESS` | C++ | 0x09 |
| (не используется) | `UnitOrderState::State10` | `ORDER_STATE_10` | C++/RE | 0x0A; не используется ни в C++-порте, ни в оригинале (RE: ни присваивания, ни сравнения) |
| Строительство | `UnitOrderState::BuildInProgress` | `ORDER_STATE_BUILD_IN_PROGRESS` | C++ | 0x0B |
| Отмена запроса пути | `UnitOrderState::PathRequestCancel` | `ORDER_STATE_PATH_REQUEST_CANCEL` | C++ | 0x0C |
| Отмена стройки | `UnitOrderState::BuildCancel` | `ORDER_STATE_BUILD_CANCEL` | C++ | 0x0D |
| Уничтожение | `UnitOrderState::Destroy` | `ORDER_STATE_DESTROY` | C++ | 0x0E |
| Погрузка | `UnitOrderState::LoadingInProgress` | `ORDER_STATE_LOADING_IN_PROGRESS` | C++ | 0x0F |
| Погрузка завершена | `UnitOrderState::FinishLoading` | `ORDER_STATE_FINISH_LOADING` | C++ | 0x10 |
| Выгрузка | `UnitOrderState::UnloadingInProgress` | `ORDER_STATE_UNLOADING_IN_PROGRESS` | C++ | 0x11 |
| Выгрузка завершена | `UnitOrderState::FinishUnloading` | `ORDER_STATE_FINISH_UNLOADING` | C++ | 0x12 |
| Посадка завершена | `UnitOrderState::FinishLanding` | `ORDER_STATE_FINISH_LANDING` | C++ | 0x13 |
| Взлёт завершён | `UnitOrderState::FinishTakeOff` | `ORDER_STATE_FINISH_TAKE_OFF` | C++ | 0x14 |
| (не используется) | `UnitOrderState::State21` | `ORDER_STATE_21` | C++/RE | 0x15; не используется ни в C++-порте, ни в оригинале (RE: ни присваивания, ни сравнения) |
| Трансформация | `UnitOrderState::ProgressTransforming` | `ORDER_STATE_PROGRESS_TRANSFORMING` | C++ | 0x16 |
| Трансформация завершена | `UnitOrderState::FinishTransforming` | `ORDER_STATE_FINISH_TRANSFORMING` | C++ | 0x17 |
| Расчистка пути | `UnitOrderState::ClearPath` | `ORDER_STATE_CLEAR_PATH` | C++ | 0x18 |
| Выбор места | `UnitOrderState::SelectSite` | `ORDER_STATE_SELECT_SITE` | C++ | 0x19 |
| Расчистка под стройку | `UnitOrderState::BuildClearing` | `ORDER_STATE_BUILD_CLEARING` | C++ | 0x1A |
| Взрыв | `UnitOrderState::Explode` | `ORDER_STATE_EXPLODE` | C++ | 0x1B |
| Начало движения | `UnitOrderState::MoveInit` | `ORDER_STATE_MOVE_INIT` | C++ | 0x1C |
| Получение пути | `UnitOrderState::MoveGettingPath` | `ORDER_STATE_MOVE_GETTING_PATH` | C++ | 0x1D |
| Здание готово | `UnitOrderState::BuildingReady` | `ORDER_STATE_BUILDING_READY` | C++ | 0x1E |
| Юнит готов | `UnitOrderState::UnitReady` | `ORDER_STATE_UNIT_READY` | C++ | 0x1F |
| Развёртывание | `UnitOrderState::Expand` | `ORDER_STATE_EXPAND` | C++ | 0x20 |
| Сворачивание | `UnitOrderState::Shrink` | `ORDER_STATE_SHRINK` | C++ | 0x21 |
| Инициализация ленты | `UnitOrderState::TapePositioningInit` | `ORDER_STATE_TAPE_POSITIONING_INIT` | C++ | 0x22 |
| Вход в зону ленты | `UnitOrderState::TapePositioningEnter` | `ORDER_STATE_TAPE_POSITIONING_ENTER` | C++ | 0x23 |
| Деинициализация ленты | `UnitOrderState::TapePositioningDeinit` | `ORDER_STATE_TAPE_POSITIONING_DEINIT` | C++ | 0x24 |
| Выход из зоны ленты | `UnitOrderState::TapePositioningLeave` | `ORDER_STATE_TAPE_POSITIONING_LEAVE` | C++ | 0x25 |
| Подъём | `UnitOrderState::Elevate` | `ORDER_STATE_ELEVATE` | C++ | 0x26; мост |
| Опускание | `UnitOrderState::Lower` | `ORDER_STATE_LOWER` | C++ | 0x27; мост |
| Атака ожидает | `UnitOrderState::AttackPending` | `ORDER_STATE_ATTACK_PENDING` | C++ | 0x28 |
| Начало атаки | `UnitOrderState::AttackBeginning` | `ORDER_STATE_ATTACK_BEGINNING` | C++ | 0x29 |
| Новый приказ | `UnitOrderState::NewOrder` | `ORDER_STATE_NEW_ORDER` | C++ | 0x2A |
| Неактивен | `UnitOrderState::Inactive` | `ORDER_STATE_INACTIVE` | C++ | 0x2B |
| Установка мин | `UnitOrderState::PlacingMines` | `ORDER_STATE_PLACING_MINES` | C++ | 0x2C |
| Снятие мин | `UnitOrderState::RemovingMines` | `ORDER_STATE_REMOVING_MINES` | C++ | 0x2D |
| Прерывание стройки | `UnitOrderState::BuildAbort` | `ORDER_STATE_BUILD_ABORT` | C++ | 0x2E |

## Ресурсы

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
| raw (сырьё / материалы) | `ResourceKind::Raw`, `Cargo.raw`, `Complex.raw` | `CARGO_TYPE_RAW`, `CARGO_MATERIALS` | C++/мануал | добыча `MININGST`; тратится заводами (3/ход), строителями (2/ход), `TRAINHAL` (1/ход); склад `ADUMP` |
| fuel (топливо) | `ResourceKind::Fuel`, `Cargo.fuel`, `Complex.fuel` | `CARGO_TYPE_FUEL`, `CARGO_FUEL` | C++ | потребляется только электростанциями (`POWERSTN` 6, `POWGEN` 2); склад `FDUMP` |
| gold (золото, руда) | `ResourceKind::Gold`, `Cargo.gold`, `Complex.gold` | `CARGO_TYPE_GOLD`, `CARGO_GOLD` | C++ | перерабатывается `COMMTWR` 5/ход → кредиты; склад `GOLDSM` |
| power (энергия / мощность) | `Complex.power` | `Complex.power` | C++ | производится `POWERSTN`/`POWGEN`; потребляется по 1/ход заводами/исследованиями; не хранится |
| workers (рабочие) | `Complex.workers` | `Complex.workers` | C++/мануал/RE | канон по оригиналу «workers»; C++-токен `Cargo.life` — ложный друг (≠ прочность `hits`); производится `HABITAT` (+3), тратится `TRAINHAL`/`GREENHSE`/`RESEARCH` |
| credits (кредиты) | `TeamUnits::gold` | `TeamUnits::gold` | C++/мануал | очищенное золото на счёте команды; тратится на покупку/апгрейд; не путать с `gold` (руда) |

> Примечания: грузовой тип юнита — `CargoType` (`None/Raw/Fuel/Gold/Land/Sea/Air`);
> вместимость — `ATTRIB_STORAGE`; собственный запас топлива машины — `ATTRIB_FUEL`
> (не путать с ресурсом «топливо»). `ResourceKind` покрывает только 3 рудных
> ресурса карты; энергия/рабочие/кредиты — отдельные оси. Rust-`Cargo`
> (`raw/fuel/gold/free_capacity`) сужен относительно C++ `cargo.hpp`
> (`power`/`life`); **подтверждено RE**: `power`/`life` — дельты баланса комплекса
> (`Complex.power`/`Complex.workers`), а не груз транспорта — сужение семантически
> точно. C++-токен `life` в оригинале называется «workers» (рабочие).

## Команды и кланы

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|

## Прочее

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
