# Domain Map — соответствие доменного языка и C++-оракула

Каноническая таблица соответствия «домен-термин → Rust-имя → C++-идентификатор
→ источник → примечание». Единый язык домена (DSL) для аналитиков, дизайнеров,
программистов и архитекторов. Пополняется при введении каждой новой сущности
(правило 6 в [`development/rust-programming-rules.md`](development/rust-programming-rules.md)).

Формат строки:

| Домен-термин | Rust-имя | C++-идентификатор | Источник (файл:строка) | Примечание |
|---|---|---|---|---|

## Юниты

Каноничные DSL-имена (источник — мануал M.A.X. + C++-оракул + решения владельца).
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
| Артиллерийская башня | `ArtilleryTurret` | `ARTYTRRT` | — |
| Противоракетная установка | `AntiMissile` | `ANTIMSSL` | мануал: «Missile Launcher» |
| Бетонный блок | `Block` | `BLOCK` | — |
| Мост | `Bridge` | `BRIDGE` | — |
| Шахта | `MiningStation` | `MININGST` | — |
| Наземная мина | `LandMine` | `LANDMINE` | — |
| Морская мина | `SeaMine` | `SEAMINE` | — |
| Конструктор | `Constructor` | `CONSTRCT` | — |
| Разведчик (парящий) | `Scout` | `SCOUT` | — |
| Танк | `Tank` | `TANK` | — |
| Штурмовое орудие | `AssaultGun` | `ARTILLRY` | — |
| Ракетная установка | `RocketLauncher` | `ROCKTLCH` | — |
| Ракетный транспортёр | `MissileCrawler` | `MISSLLCH` | — |
| Мобильная ПВО | `MobileAntiAir` | `SP_FLAK` | — |
| Миноукладчик | `MineLayer` | `MINELAYR` | — |
| Сюрвейер | `Surveyor` | `SURVEYOR` | — |
| Мобильный сканер | `MobileScanner` | `SCANNER` | переименовать `Scanner` |
| Грузовик снабжения | `SupplyTruck` | `SPLYTRCK` | — |
| Золотой грузовик | `GoldTruck` | `GOLDTRCK` | — |
| Инженер | `Engineer` | `ENGINEER` | — |
| Бульдозер | `Bulldozer` | `BULLDOZR` | — |
| Ремонтный юнит | `RepairUnit` | `REPAIR` | — |
| Топливный грузовик | `FuelTruck` | `FUELTRCK` | — |
| Бронетранспортёр | `ArmouredPersonnelCarrier` | `CLNTRANS` | переименовать `PersonnelCarrier` |
| Диверсант | `Commando` | `COMMANDO` | — |
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

## Состояния

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|

## Ресурсы

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|

## Команды и кланы

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|

## Прочее

| Домен-термин | Rust-имя | C++-идентификатор | Источник | Примечание |
|---|---|---|---|---|
