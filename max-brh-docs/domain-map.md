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
| Монопольная шахта | `MonopoleMine` | `SHIELDGN` | переименовать `ShieldGenerator`; рудимент Interplay (alien derelict); токен/ассет `SHLDGEN.FLC` — пережиток «Shield GeNerator» |
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
| Завод поддержки | `SupportPlant` | `SUPRTPLT` | рудимент Interplay (alien derelict); каноничного имени нет, намек на Life Support (`I_LIFESP`) |
| Авиазавод | `AirUnitsPlant` | `AIRPLT` | — |
| Жилище | `Habitat` | `HABITAT` | — |
| Исследовательский центр | `ResearchCentre` | `RESEARCH` | переименовать `ResearchCenter` |
| Экосфера | `EcoSphere` | `GREENHSE` | переименовать `Greenhouse` |
| Центр отдыха | `RecreationCenter` | `RECCENTR` | рудимент Interplay (alien derelict) |
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

Кланы (`TeamClan`): 9 значений = 8 игровых кланов + `Random` (0). «Alien» — это
**не клан**, а слот команды (`TeamIndex::Alien` / `TeamId::ALIEN`). У всех кланов
поле `credits` из `SC_CLANS` = 0 (стартовые кредиты задаёт опция «Starting Credit»,
0–250, а не клан).

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
| Случайный клан | `TeamClan::Random` | `TEAM_CLAN_RANDOM` (id `"Random"`) | enums.rs:214 / enums.hpp:171 | 0x00; псевдо-выбор — при старте партии заменяется случайным кланом 1–8 (`Randomizer_Generate(8)+1`) |
| Избранные | `TeamClan::TheChosen` | `TEAM_CLAN_THE_CHOSEN` (id `"Clan A"`) | enums.rs:216 / manual.txt:700 | EN «The Chosen»; авиация |
| Багровый путь | `TeamClan::CrimsonPath` | `TEAM_CLAN_CRIMSON_PATH` (id `"Clan B"`) | enums.rs:217 / manual.txt:708 | EN «Crimson Path»; флот |
| Фон Гриффин | `TeamClan::VonGriffin` | `TEAM_CLAN_VON_GRIFFIN` (id `"Clan C"`) | enums.rs:218 / manual.txt:717 | EN «Von Griffin»; разведка/скрытность |
| Рука Айера | `TeamClan::AyersHand` | `TEAM_CLAN_AYERS_HAND` (id `"Clan D"`) | enums.rs:219 / manual.txt:729 | EN «Ayers Hand» (без апострофа; в мануале — «Ayer’s Hand»); ракетные юниты |
| Мусаси | `TeamClan::Musashi` | `TEAM_CLAN_MUSASHI` (id `"Clan E"`) | enums.rs:220 / manual.txt:736 | EN «Musashi»; тяжёлая броня |
| Священная восьмёрка | `TeamClan::SacredEight` | `TEAM_CLAN_SACRED_EIGHTS` (id `"Clan F"`) | enums.rs:221 / manual.txt:745 | EN «Sacred Eight» (ед. ч.); быстрая атака |
| Семь рыцарей | `TeamClan::SevenKnights` | `TEAM_CLAN_7_KNIGHTS` (id `"Clan G"`) | enums.rs:222 / manual.txt:756 | EN «Seven Knights»; стационарная оборона |
| Ось | `TeamClan::AxisInc` | `TEAM_CLAN_AXIS_INC` (id `"Clan H"`) | enums.rs:223 / manual.txt:763 | EN «Axis Inc.»; производство |

> Идентификаторы `"Clan A"…"Clan H"` — ключи ресурса `SC_CLANS`
> (`resource_manager.cpp:90-98`); отображаемые имена кланов хранятся в блоке
> `name` самого ресурса `SC_CLANS` (зашифрован в `PATCHES.RES`).

Короткая сводка бонусов (`description.en-US` из `SC_CLANS`): The Chosen — «Better
Air Units», Crimson Path — «Better Sea Units», Von Griffin — «Improved
Espionage», Ayer's Hand — «Better Missile Units», Musashi — «Better Armored
Vehicles», Sacred Eight — «Better Fast Units», Seven Knights — «Better Defense»,
Axis Inc. — «Extra constructors and engineers». Полных лор-описаний в `SC_CLANS`
**нет** — они в мануале (`manual.txt` L700–769, раздел «CLAN ORIGINS»).

### Команды (TeamKind / TeamIndex / Opponent)

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
| Пусто (нет игрока) | `TeamKind::None` | `TEAM_TYPE_NONE` | enums.rs:146 / enums.hpp:164 | 0; пустой слот команды |
| Игрок (локальный) | `TeamKind::Player` | `TEAM_TYPE_PLAYER` | enums.rs:147 / enums.hpp:165 | 1; человек за этим клиентом (в hot-seat — любой человек) |
| Компьютер | `TeamKind::Computer` | `TEAM_TYPE_COMPUTER` | enums.rs:148 / enums.hpp:166 | 2; ИИ симулируется сервером |
| Удалённый игрок | `TeamKind::Remote` | `TEAM_TYPE_REMOTE` | enums.rs:149 / enums.hpp:167 | 3; другой клиент в сети |
| Выбывший | `TeamKind::Eliminated` | `TEAM_TYPE_ELIMINATED` | enums.rs:150 / enums.hpp:168 | 4; команда уничтожена |
| Красная команда | `TeamIndex::Red` / `TeamId::RED` | `PLAYER_TEAM_RED` | enums.rs:170 / enums.hpp:155 | 0x00; бит цвета `HASH_TEAM_RED` 0x2000 |
| Зелёная команда | `TeamIndex::Green` / `TeamId::GREEN` | `PLAYER_TEAM_GREEN` | enums.rs:171 / enums.hpp:156 | 0x01; `HASH_TEAM_GREEN` 0x1000 |
| Синяя команда | `TeamIndex::Blue` / `TeamId::BLUE` | `PLAYER_TEAM_BLUE` | enums.rs:172 / enums.hpp:157 | 0x02; `HASH_TEAM_BLUE` 0x800 |
| Серая команда | `TeamIndex::Gray` / `TeamId::GRAY` | `PLAYER_TEAM_GRAY` | enums.rs:173 / enums.hpp:158 | 0x03; `HASH_TEAM_GRAY` 0x400 |
| Пришельцы (нейтралы) | `TeamIndex::Alien` / `TeamId::ALIEN` | `PLAYER_TEAM_ALIEN` | enums.rs:174 / enums.hpp:159 | 0x04; псевдо-команда нейтральных/брошенных юнитов; `HASH_TEAM_DERELICT` 0x8000 |
| (счётчик команд) | `TeamId::MAX` | `PLAYER_TEAM_MAX` | enums.rs:179 / enums.hpp:160 | 5; не команда, а верхняя граница |
| Новичок (сложность ИИ) | `Opponent::Clueless` | `OPPONENT_TYPE_CLUELESS` | enums.rs:233 / enums.hpp:184 | 0; ИИ: нет продвинутых стратегий, штраф −25 % к стройке/добыче |
| Ученик | `Opponent::Apprentice` | `OPPONENT_TYPE_APPRENTICE` | enums.rs:234 / enums.hpp:185 | 1; базовый ИИ, без штрафов |
| Средний | `Opponent::Average` | `OPPONENT_TYPE_AVERAGE` | enums.rs:235 / enums.hpp:186 | 2; продвинутые стратегии |
| Эксперт | `Opponent::Expert` | `OPPONENT_TYPE_EXPERT` | enums.rs:236 / enums.hpp:187 | 3; умнейший доступный ИИ |
| Мастер | `Opponent::Master` | `OPPONENT_TYPE_MASTER` | enums.rs:237 / enums.hpp:188 | 4; +25 % к производству и добыче |
| Бог | `Opponent::God` | `OPPONENT_TYPE_GOD` | enums.rs:238 / enums.hpp:189 | 5; +50 % к производству и добыче |

### Режимы, победа, категории миссий

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
| Пошаговый режим | `PlayMode::TurnBased` | `PLAY_MODE_TURN_BASED` | enums.rs:248 / enums.hpp:224 | 0; каждый игрок ходит по очереди |
| Одновременный режим | `PlayMode::SimultaneousMoves` | `PLAY_MODE_SIMULTANEOUS_MOVES` | enums.rs:249 / enums.hpp:225 | 1; все ходят одновременно (real-time внутри хода) |
| (не определено) | `PlayMode::Unknown` | `PLAY_MODE_UNKNOWN` | enums.rs:250 / enums.hpp:226 | 2; служебный страж при загрузке/настройке, не UI-вариант |
| Победа по числу ходов | `VictoryType::Duration` | `VICTORY_TYPE_DURATION` | enums.rs:257 / enums.hpp:219 | 0; по достижении лимита ходов побеждает игрок с максимумом очков |
| Победа по очкам | `VictoryType::Score` | `VICTORY_TYPE_SCORE` | enums.rs:258 / enums.hpp:220 | 1; первый, достигший цели по очкам (1 очко/ход за работающую `GREENHSE`), побеждает |
| (категория миссии) | `MissionCategory::Custom`… | `MISSION_CATEGORY_CUSTOM`… | victory.rs:13 / mission.hpp:31 | не `PlayMode`; 8 значений: Custom/Training/Campaign/HotSeat/Multiplayer/Demo/Scenario/MultiPlayerScenario |
| (тип сохранения) | `FileType::Custom`… | save.md `FileType` | enums.rs:264 / save.md | 10 значений заголовка V70; маппинг в `MissionCategory::from_file_type` |

### Исследования (ResearchTopic)

Порядок тем совпадает с первыми 8 атрибутами `ATTRIB_*` (`unit_values.rs:38-52`):
каждая тема улучшает соответствующий атрибут всех новых юнитов.

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
| Атака | `ResearchTopic::Attack` | `RESEARCH_TOPIC_ATTACK` | enums.rs:316 / enums.hpp:237 | 0 = `ATTRIB_ATTACK` |
| Выстрелы | `ResearchTopic::Shots` | `RESEARCH_TOPIC_SHOTS` | enums.rs:317 / enums.hpp:238 | 1 = `ATTRIB_ROUNDS` |
| Дальность | `ResearchTopic::Range` | `RESEARCH_TOPIC_RANGE` | enums.rs:318 / enums.hpp:239 | 2 = `ATTRIB_RANGE` |
| Броня | `ResearchTopic::Armor` | `RESEARCH_TOPIC_ARMOR` | enums.rs:319 / enums.hpp:240 | 3 = `ATTRIB_ARMOR` |
| Прочность | `ResearchTopic::Hits` | `RESEARCH_TOPIC_HITS` | enums.rs:320 / enums.hpp:241 | 4 = `ATTRIB_HITS` |
| Скорость | `ResearchTopic::Speed` | `RESEARCH_TOPIC_SPEED` | enums.rs:321 / enums.hpp:242 | 5 = `ATTRIB_SPEED` |
| Сканирование | `ResearchTopic::Scan` | `RESEARCH_TOPIC_SCAN` | enums.rs:322 / enums.hpp:243 | 6 = `ATTRIB_SCAN` |
| Стоимость | `ResearchTopic::Cost` | `RESEARCH_TOPIC_COST` | enums.rs:323 / enums.hpp:244 | 7 = `ATTRIB_TURNS`; сокращает время постройки |

### Характеристики кланов (бонусы/штрафы из `SC_CLANS`)

Полная таблица — `CLAN_TRADEOFFS` в `max-brh/crates/max_core/src/unit_values_tables.in:200`
(кортежи `(клан, юнит, атрибут, дельта)`, 95 записей; применяются в
`ResourceManager_SetClanUpgradesFromClans` / `unit_values::apply_clan_tradeoffs`).

| Клан | Архетип | Бонусы (юнит → атрибуты) |
|---|---|---|
| Избранные (The Chosen) | авиация | `AIRPLT` −5 ходов постройки; `Bomber` +4 атака, −1 ход; `Fighter` +1 дальность, −1 ход; `AirTransport` +4 скорость; `AWAC` +2 скан |
| Багровый путь (Crimson Path) | флот | `SHIPYARD` −5 ходов; `Submarine` +4 атака, +2 скорость, −1 ход; `Gunboat` +2 броня, +4 прочность, +1 дальность; `Escort`/`Corvette`/`SeaTransport`/`CargoShip` +2 скорость; `MissileCruiser` +1 дальность, −1 ход |
| Фон Гриффин (Von Griffin) | разведка | `Radar` +6 скан; `Scout` +2 атака/скорость/скан; `Scanner` +4 скан, +1 скорость; `AWAC` +4 скан, +2 скорость; `Submarine` +4 атака, +2 скорость, +2 скан; `Infiltrator` +1 скан/скорость, +2 опыт, −1 ход; `APC` +1 броня, +2 прочность, +2 скорость |
| Рука Айера (Ayers Hand) | ракеты | `RocketLauncher`, `MissileCrawler`, `MissileLauncher` (турель), `MissileCruiser`: +2 атака, +1 дальность, −2 хода |
| Мусаси (Musashi) | тяжёлая броня | `Tank` +4 атака, +2 броня, +4 прочность, +1 скорость; `GunTurret` +4/+2/+4; `Gunboat` +4 атака, +2 броня, +8 прочность, +1 дальность; `APC` +2 броня, +4 прочность, +1 скорость |
| Священная восьмёрка (Sacred Eight) | быстрая атака | `AssaultGun` +4 атака, +2 скорость, +1 дальность, −2 хода; `Scout` +2 атака, +2 скорость; `Bomber` +2 атака, +2 скорость; `MobileAntiAir` +1 дальность, +2 скорость; `Escort` +1 дальность, +2 скорость; `Corvette` +2 атака, +2 скорость |
| Семь рыцарей (Seven Knights) | оборона | `GunTurret` +4 атака, +2 броня, +4 прочность; `Artillery` +2 атака, +1 дальность; `AntiAir` +2 атака, +1 дальность; `MissileLauncher` +2 атака, +1 дальность, −1 ход; `LandMine` +8 атака; `SeaMine` +8 атака; `Radar` +2 скан |
| Ось (Axis Inc.) | производство | `LIGHTPLT` −2 хода, `LANDPLT` −4 хода постройки |

> Архетипы подтверждаются маппингом клан→ИИ-стратегия в `ai/strategy.rs:1233-1237`
> и тестами (`team.rs`, `production.rs`, `unit_values.rs`).

## Прочее

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
