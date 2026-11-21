# Финальное ревью имён юнитов

Сверка «Rust (код) / C++ / мануал / User-Friendly Name». C++-идентификаторы — из
doc-комментариев `UnitType` (`/// TOKEN / C++_NAME`); мануал — `MANUAL.PDF`.
Эффекты/декорации/alien мануал не описывает (в столбце «Мануал» — «—»).

> **Указание разработчику.**
> - В **коде** используется имя из колонки **Rust** (идентификатор `UnitType`).
> - В **интерфейсе игры** отображается имя из колонки **User-Friendly Name**
>   (английская строка, как в оригинальном мануале/игре).
> - Переименования текущего кода сведены в раздел «Переименования» ниже.

## Юниты и здания

| № | Токен | Rust (код) | C++ | Мануал | User-Friendly Name |
|---|---|---|---|---|---|
| 1 | COMMTWR | GoldRefinery | UNIT_TYPE_GOLD_REFINERY | Gold Refinery | Gold Refinery |
| 2 | POWERSTN | PowerStation | POWER_STATION | Power Stations | Power Station |
| 3 | POWGEN | PowerGenerator | POWER_GENERATOR | Power Generators | Power Generator |
| 4 | BARRACKS | Barracks | BARRACKS | Barracks | Barracks |
| 5 | SHIELDGN | ShieldGenerator | ALIEN_BUILDING_1 | — | Shield Generator |
| 6 | RADAR | Radar | RADAR | Radar | Radar |
| 7 | ADUMP | MaterialStorage | STORAGE_UNIT | Material Storage | Material Storage |
| 8 | FDUMP | FuelStorage | FUEL_TANK | Fuel Storage | Fuel Storage |
| 9 | GOLDSM | GoldVault | GOLD_VAULT | Gold Vault | Gold Vault |
| 10 | DEPOT | Depot | DEPOT | Depot | Depot |
| 11 | HANGAR | Hangar | HANGAR | Hangar | Hangar |
| 12 | DOCK | Dock | DOCK | Dock | Dock |
| 13 | CNCT_4W | Connector | CONNECTOR | Connectors | Connector |
| 14 | ROAD | Road | ROAD | Road | Road |
| 15 | LANDPAD | LandingPad | LANDING_PAD | Landing Pad | Landing Pad |
| 16 | SHIPYARD | Shipyard | SHIPYARD | Shipyard | Shipyard |
| 17 | LIGHTPLT | LightVehiclePlant | LIGHT_VEHICLE_PLANT | Light Vehicle Plant | Light Vehicle Plant |
| 18 | LANDPLT | HeavyVehiclePlant | HEAVY_VEHICLE_PLANT | Heavy Vehicle Plant | Heavy Vehicle Plant |
| 19 | SUPRTPLT | SupportPlant | ALIEN_BUILDING_2 | — | Support Plant |
| 20 | AIRPLT | AirUnitsPlant | AIR_UNITS_PLANT | Air Units Plant | Air Units Plant |
| 21 | HABITAT | Habitat | HABITAT | Habitat | Habitat |
| 22 | RESEARCH | ResearchCentre | RESEARCH_CENTER | Research Centre | Research Centre |
| 23 | GREENHSE | EcoSphere | ECOSPHERE | Eco-Sphere | Eco-Sphere |
| 24 | RECCENTR | RecreationCenter | ALIEN_BUILDING_3 | — | Recreation Center |
| 25 | TRAINHAL | TrainingHall | TRAINING_HALL | Training Hall | Training Hall |
| 26 | WTRPLTFM | WaterPlatform | WATER_PLATFORM | Water Platform | Water Platform |
| 27 | GUNTURRT | GunTurret | GUN_TURRET | Gun Turret | Gun Turret |
| 28 | ANTIAIR | AntiAir | ANTI_AIRCRAFT | Anti-Air | Anti-Air |
| 29 | ARTYTRRT | Artillery | ARTILLERY | Artillery | Artillery |
| 30 | ANTIMSSL | MissileLauncher | MISSILE_LAUNCHER | Missile Launcher | Missile Launcher |
| 31 | BLOCK | ConcreteBlock | CONCRETE_BLOCK | Concrete Block | Concrete Block |
| 32 | BRIDGE | Bridge | BRIDGE | Bridge | Bridge |
| 33 | MININGST | MiningStation | MINING_STATION | Mining Stations | Mining Station |
| 34 | LANDMINE | LandMine | LAND_MINE | — | Land Mine |
| 35 | SEAMINE | SeaMine | SEA_MINE | — | Sea Mine |
| 36 | CONSTRCT | Constructor | CONSTRUCTOR | Constructors | Constructor |
| 37 | SCOUT | Scout | SCOUT | Scouts | Scout |
| 38 | TANK | Tank | TANK | Tanks | Tank |
| 39 | ARTILLRY | AssaultGun | ASSAULT_GUN | Assault Guns | Assault Gun |
| 40 | ROCKTLCH | RocketLauncher | ROCKET_LAUNCHER | Rocket Launcher | Rocket Launcher |
| 41 | MISSLLCH | MissileCrawler | MISSILE_CRAWLER | Missile Crawlers | Missile Crawler |
| 42 | SP_FLAK | MobileAntiAir | MOBILE_ANTI_AIRCRAFT | Mobile Anti-Air | Mobile Anti-Air |
| 43 | MINELAYR | MineLayer | MINE_LAYER | Mine Layers | Mine Layer |
| 44 | SURVEYOR | Surveyor | SURVEYOR | Surveyors | Surveyor |
| 45 | SCANNER | Scanner | SCANNER | Mobile Scanners | Scanner |
| 46 | SPLYTRCK | SupplyTruck | SUPPLY_TRUCK | Supply Trucks | Supply Truck |
| 47 | GOLDTRCK | GoldTruck | GOLD_TRUCK | Gold Truck | Gold Truck |
| 48 | ENGINEER | Engineer | ENGINEER | Engineers | Engineer |
| 49 | BULLDOZR | Bulldozer | BULLDOZER | Bulldozers | Bulldozer |
| 50 | REPAIR | RepairUnit | REPAIR_UNIT | Repair Units | Repair Unit |
| 51 | FUELTRCK | FuelTruck | FUEL_TRUCK | Fuel Truck | Fuel Truck |
| 52 | CLNTRANS | ArmouredPersonnelCarrier | PERSONNEL_CARRIER | Armoured Personnel Carriers | Armoured Personnel Carrier |
| 53 | COMMANDO | Infiltrator | INFILTRATOR | Infiltrator | Infiltrator |
| 54 | INFANTRY | Infantry | INFANTRY | Infantry | Infantry |
| 55 | FASTBOAT | Escort | ESCORT | Escorts | Escort |
| 56 | CORVETTE | Corvette | CORVETTE | Corvette | Corvette |
| 57 | BATTLSHP | Gunboat | GUNBOAT | Gunboats | Gunboat |
| 58 | SUBMARNE | Submarine | SUBMARINE | Submarines | Submarine |
| 59 | SEATRANS | SeaTransport | SEA_TRANSPORT | Supply Boats | Sea Transport |
| 60 | MSSLBOAT | MissileCruiser | MISSILE_CRUISER | Missile Cruisers | Missile Cruiser |
| 61 | SEAMNLYR | SeaMineLayer | SEA_MINE_LAYER | Sea Mine Layer | Sea Mine Layer |
| 62 | CARGOSHP | CargoShip | CARGO_SHIP | Supply Boats | Cargo Ship |
| 63 | FIGHTER | Fighter | FIGHTER | Fighters | Fighter |
| 64 | BOMBER | Bomber | GROUND_ATTACK_PLANE | Ground Attack Planes | Ground Attack Plane |
| 65 | AIRTRANS | AirTransport | AIR_TRANSPORT | Air Transports | Air Transport |
| 66 | AWAC | Awac | AWAC | AWAC | AWAC |
| 67 | JUGGRNT | Juggernaut | ALIEN_GUNBOAT | — | Juggernaut |
| 68 | ALNTANK | AlienTank | ALIEN_TANK | — | Alien Tank |
| 69 | ALNASGUN | AlienAssaultGun | ALIEN_ASSAULT_GUN | — | Alien Assault Gun |
| 70 | ALNPLANE | AlienPlane | ALIEN_PLANE | — | Alien Plane |

## Переименования (для разработчика)

Текущее имя `UnitType` → каноничное (колонка Rust выше):

| Текущее | Каноничное |
|---|---|
| CommTower | GoldRefinery |
| Greenhouse | EcoSphere |
| ADump | MaterialStorage |
| FDump | FuelStorage |
| GoldSmelter | GoldVault |
| ResearchCenter | ResearchCentre |
| ArtilleryTurret | Artillery |
| AntiMissile | MissileLauncher |
| Block | ConcreteBlock |
| PersonnelCarrier | ArmouredPersonnelCarrier |
| Commando | Infiltrator |
| FastBoat | Escort |
| Battleship | Gunboat |
| MissileBoat | MissileCruiser |

## Открытые вопросы (требуют решения владельца)

- `SHIELDGN` / `SUPRTPLT` / `RECCENTR`: C++-идентификаторы — `ALIEN_BUILDING_1/2/3`.
  Требуется уточнить каноничные имена (сейчас `ShieldGenerator` / `SupportPlant` /
  `RecreationCenter`).
- `BOMBER`: Rust-имя `Bomber` (решение владельца), User-Friendly Name — «Ground Attack Plane».
- `SEATRANS`: мануал — «Supply Boats»; решение владельца — «Sea Transport».
- `CARGOSHP`: мануал — «Supply Boats» (совпадает с `SEATRANS` в списке верфи);
  уточнить корректный User-Friendly Name.
