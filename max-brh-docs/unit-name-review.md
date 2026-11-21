# Финальное ревью имён юнитов

Сверка «у нас (Rust) / в C++ / в мануале / по-русски». C++-идентификаторы — из
doc-комментариев `UnitType` (`/// TOKEN / C++_NAME`); мануал — `MANUAL.PDF`;
по-русски — каноничный русский DSL-термин. Эффекты/декорации/alien мануал не
описывает (в столбце «Мануал» — «—»).

## Юниты и здания

| Токен | Rust (у нас) | C++ | Мануал | По-русски |
|---|---|---|---|---|
| COMMTWR | CommTower | UNIT_TYPE_GOLD_REFINERY | Gold Refinery | очиститель золота |
| POWERSTN | PowerStation | POWER_STATION | Power Stations | электростанция |
| POWGEN | PowerGenerator | POWER_GENERATOR | Power Generators | энергогенератор |
| BARRACKS | Barracks | BARRACKS | Barracks | казарма |
| SHIELDGN | ShieldGenerator | ALIEN_BUILDING_1 | — | щитовой генератор (alien-здание 1) |
| RADAR | Radar | RADAR | Radar | радар |
| ADUMP | ADump | STORAGE_UNIT | Material Storage | склад материалов |
| FDUMP | FDump | FUEL_TANK | Fuel Storage | склад топлива |
| GOLDSM | GoldSmelter | GOLD_VAULT | Gold Vault | золотое хранилище |
| DEPOT | Depot | DEPOT | Depot | депо (ремонт/апгрейд) |
| HANGAR | Hangar | HANGAR | Hangar | ангар |
| DOCK | Dock | DOCK | Dock | док |
| CNCT_4W | Connector | CONNECTOR | Connectors | коннектор |
| ROAD | Road | ROAD | Road | дорога |
| LANDPAD | LandingPad | LANDING_PAD | Landing Pad | посадочная площадка |
| SHIPYARD | Shipyard | SHIPYARD | Shipyard | верфь |
| LIGHTPLT | LightVehiclePlant | LIGHT_VEHICLE_PLANT | Light Vehicle Plant | завод лёгкой техники |
| LANDPLT | HeavyVehiclePlant | HEAVY_VEHICLE_PLANT | Heavy Vehicle Plant | завод тяжёлой техники |
| SUPRTPLT | SupportPlant | ALIEN_BUILDING_2 | — | завод поддержки (alien-здание 2) |
| AIRPLT | AirUnitsPlant | AIR_UNITS_PLANT | Air Units Plant | авиазавод |
| HABITAT | Habitat | HABITAT | Habitat | жилище |
| RESEARCH | ResearchCenter | RESEARCH_CENTER | Research Centre | исследовательский центр |
| GREENHSE | Greenhouse | ECOSPHERE | Eco-Sphere | экосфера |
| RECCENTR | RecreationCenter | ALIEN_BUILDING_3 | — | центр отдыха (alien-здание 3) |
| TRAINHAL | TrainingHall | TRAINING_HALL | Training Hall | тренировочный зал |
| WTRPLTFM | WaterPlatform | WATER_PLATFORM | Water Platform | водная платформа |
| GUNTURRT | GunTurret | GUN_TURRET | Gun Turret | орудийная башня |
| ANTIAIR | AntiAir | ANTI_AIRCRAFT | Anti-Air | зенитка |
| ARTYTRRT | ArtilleryTurret | ARTILLERY | Artillery | артиллерийская башня |
| ANTIMSSL | AntiMissile | MISSILE_LAUNCHER | Missile Launcher | противоракетная/ракетная установка |
| BLOCK | Block | CONCRETE_BLOCK | Concrete Block | бетонный блок |
| BRIDGE | Bridge | BRIDGE | Bridge | мост |
| MININGST | MiningStation | MINING_STATION | Mining Stations | шахта |
| LANDMINE | LandMine | LAND_MINE | — | наземная мина |
| SEAMINE | SeaMine | SEA_MINE | — | морская мина |
| CONSTRCT | Constructor | CONSTRUCTOR | Constructors | конструктор |
| SCOUT | Scout | SCOUT | Scouts | разведчик (парящий) |
| TANK | Tank | TANK | Tanks | танк |
| ARTILLRY | AssaultGun | ASSAULT_GUN | Assault Guns | штурмовое орудие |
| ROCKTLCH | RocketLauncher | ROCKET_LAUNCHER | Rocket Launcher | ракетная установка |
| MISSLLCH | MissileCrawler | MISSILE_CRAWLER | Missile Crawlers | ракетный транспортёр |
| SP_FLAK | MobileAntiAir | MOBILE_ANTI_AIRCRAFT | Mobile Anti-Air | мобильная ПВО |
| MINELAYR | MineLayer | MINE_LAYER | Mine Layers | миноукладчик |
| SURVEYOR | Surveyor | SURVEYOR | Surveyors | сюрвейер |
| SCANNER | Scanner | SCANNER | Mobile Scanners | мобильный сканер |
| SPLYTRCK | SupplyTruck | SUPPLY_TRUCK | Supply Trucks | грузовик снабжения |
| GOLDTRCK | GoldTruck | GOLD_TRUCK | Gold Truck | золотой грузовик |
| ENGINEER | Engineer | ENGINEER | Engineers | инженер |
| BULLDOZR | Bulldozer | BULLDOZER | Bulldozers | бульдозер |
| REPAIR | RepairUnit | REPAIR_UNIT | Repair Units | ремонтный юнит |
| FUELTRCK | FuelTruck | FUEL_TRUCK | Fuel Truck | топливный грузовик |
| CLNTRANS | PersonnelCarrier | PERSONNEL_CARRIER | Armoured Personnel Carriers | бронетранспортёр |
| COMMANDO | Commando | INFILTRATOR | Infiltrator | диверсант |
| INFANTRY | Infantry | INFANTRY | Infantry | пехота |
| FASTBOAT | FastBoat | ESCORT | Escorts | эскорт |
| CORVETTE | Corvette | CORVETTE | Corvette | корвет |
| BATTLSHP | Battleship | GUNBOAT | Gunboats | канонерка |
| SUBMARNE | Submarine | SUBMARINE | Submarines | подлодка |
| SEATRANS | SeaTransport | SEA_TRANSPORT | Supply Boats | морской транспорт |
| MSSLBOAT | MissileBoat | MISSILE_CRUISER | Missile Cruisers | ракетный крейсер |
| SEAMNLYR | SeaMineLayer | SEA_MINE_LAYER | Sea Mine Layer | морской миноукладчик |
| CARGOSHP | CargoShip | CARGO_SHIP | Supply Boats | грузовое судно |
| FIGHTER | Fighter | FIGHTER | Fighters | истребитель |
| BOMBER | Bomber | GROUND_ATTACK_PLANE | Ground Attack Planes | бомбардировщик |
| AIRTRANS | AirTransport | AIR_TRANSPORT | Air Transports | воздушный транспорт |
| AWAC | Awac | AWAC | AWAC | AWAC |
| JUGGRNT | Juggernaut | ALIEN_GUNBOAT | — | джаггернаут (alien) |
| ALNTANK | AlienTank | ALIEN_TANK | — | танк пришельцев |
| ALNASGUN | AlienAssaultGun | ALIEN_ASSAULT_GUN | — | штурмовое орудие пришельцев |
| ALNPLANE | AlienPlane | ALIEN_PLANE | — | самолёт пришельцев |

## Расхождения и решения

| Токен | Проблема | Каноничное решение |
|---|---|---|
| COMMTWR | Rust `CommTower` ≠ C++/мануал «Gold Refinery» | `GoldRefinery` |
| GREENHSE | Rust `Greenhouse` ≠ C++ `ECOSPHERE`/мануал «Eco-Sphere» | `EcoSphere` |
| ADUMP | Rust `ADump` ≠ C++ `STORAGE_UNIT`/мануал «Material Storage» | `MaterialStorage` |
| FDUMP | Rust `FDump` ≠ мануал «Fuel Storage» (C++ `FUEL_TANK`) | `FuelStorage` |
| GOLDSM | Rust `GoldSmelter` ≠ C++ `GOLD_VAULT`/мануал «Gold Vault» | `GoldVault` |
| ARTYTRRT | Rust `ArtilleryTurret` ≠ C++/мануал «Artillery» | `Artillery` |
| ANTIMSSL | Rust `AntiMissile` ≠ C++ `MISSILE_LAUNCHER`/мануал «Missile Launcher» | `MissileLauncher` |
| BLOCK | Rust `Block` ≠ C++/мануал «Concrete Block» | `ConcreteBlock` |
| FASTBOAT | Rust `FastBoat` ≠ C++ `ESCORT`/мануал «Escorts» | `Escort` |
| BATTLSHP | Rust `Battleship` ≠ C++ `GUNBOAT`/мануал «Gunboats» | `Gunboat` |
| MSSLBOAT | Rust `MissileBoat` ≠ C++ `MISSILE_CRUISER`/мануал «Missile Cruisers» | `MissileCruiser` |
| CLNTRANS | Rust `PersonnelCarrier` ≠ мануал «Armoured Personnel Carriers» | `ArmouredPersonnelCarrier` |
| BOMBER | C++/мануал «Ground Attack Plane» vs решение владельца | `Bomber` (бомбардировщик) |
| SEATRANS | мануал «Supply Boats» vs решение владельца | `SeaTransport` (морской транспорт) |
| RESEARCH | Rust/C++ «Center» vs мануал «Centre» | `ResearchCentre` (или `ResearchCenter`) |
| SCANNER | мануал «Mobile Scanners» vs C++ `SCANNER` | `Scanner` (или `MobileScanner`) |
| SHIELDGN/SUPRTPLT/RECCENTR | C++: `ALIEN_BUILDING_1/2/3` | уточнить у владельца |

## Примечания

- Каноничный источник для имени в коде — **C++-идентификатор** (правило 3);
  мануал — источник описательных/русских терминов. Расхождение C++ ↔ мануал
  фиксируется отдельно (как для `POWERSTN`: 6 vs 8).
- Переименование вариантов `UnitType` — задача программиста (правило 2),
  выполняемая по этой таблице.
