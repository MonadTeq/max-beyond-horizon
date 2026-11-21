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

| Токен | Rust (код) | C++ | Мануал | User-Friendly Name |
|---|---|---|---|---|
| COMMTWR | GoldRefinery | UNIT_TYPE_GOLD_REFINERY | Gold Refinery | Gold Refinery |
| POWERSTN | PowerStation | POWER_STATION | Power Stations | Power Station |
| POWGEN | PowerGenerator | POWER_GENERATOR | Power Generators | Power Generator |
| BARRACKS | Barracks | BARRACKS | Barracks | Barracks |
| SHIELDGN | ShieldGenerator | ALIEN_BUILDING_1 | — | Shield Generator |
| RADAR | Radar | RADAR | Radar | Radar |
| ADUMP | MaterialStorage | STORAGE_UNIT | Material Storage | Material Storage |
| FDUMP | FuelStorage | FUEL_TANK | Fuel Storage | Fuel Storage |
| GOLDSM | GoldVault | GOLD_VAULT | Gold Vault | Gold Vault |
| DEPOT | Depot | DEPOT | Depot | Depot |
| HANGAR | Hangar | HANGAR | Hangar | Hangar |
| DOCK | Dock | DOCK | Dock | Dock |
| CNCT_4W | Connector | CONNECTOR | Connectors | Connector |
| ROAD | Road | ROAD | Road | Road |
| LANDPAD | LandingPad | LANDING_PAD | Landing Pad | Landing Pad |
| SHIPYARD | Shipyard | SHIPYARD | Shipyard | Shipyard |
| LIGHTPLT | LightVehiclePlant | LIGHT_VEHICLE_PLANT | Light Vehicle Plant | Light Vehicle Plant |
| LANDPLT | HeavyVehiclePlant | HEAVY_VEHICLE_PLANT | Heavy Vehicle Plant | Heavy Vehicle Plant |
| SUPRTPLT | SupportPlant | ALIEN_BUILDING_2 | — | Support Plant |
| AIRPLT | AirUnitsPlant | AIR_UNITS_PLANT | Air Units Plant | Air Units Plant |
| HABITAT | Habitat | HABITAT | Habitat | Habitat |
| RESEARCH | ResearchCentre | RESEARCH_CENTER | Research Centre | Research Centre |
| GREENHSE | EcoSphere | ECOSPHERE | Eco-Sphere | Eco-Sphere |
| RECCENTR | RecreationCenter | ALIEN_BUILDING_3 | — | Recreation Center |
| TRAINHAL | TrainingHall | TRAINING_HALL | Training Hall | Training Hall |
| WTRPLTFM | WaterPlatform | WATER_PLATFORM | Water Platform | Water Platform |
| GUNTURRT | GunTurret | GUN_TURRET | Gun Turret | Gun Turret |
| ANTIAIR | AntiAir | ANTI_AIRCRAFT | Anti-Air | Anti-Air |
| ARTYTRRT | Artillery | ARTILLERY | Artillery | Artillery |
| ANTIMSSL | MissileLauncher | MISSILE_LAUNCHER | Missile Launcher | Missile Launcher |
| BLOCK | ConcreteBlock | CONCRETE_BLOCK | Concrete Block | Concrete Block |
| BRIDGE | Bridge | BRIDGE | Bridge | Bridge |
| MININGST | MiningStation | MINING_STATION | Mining Stations | Mining Station |
| LANDMINE | LandMine | LAND_MINE | — | Land Mine |
| SEAMINE | SeaMine | SEA_MINE | — | Sea Mine |
| CONSTRCT | Constructor | CONSTRUCTOR | Constructors | Constructor |
| SCOUT | Scout | SCOUT | Scouts | Scout |
| TANK | Tank | TANK | Tanks | Tank |
| ARTILLRY | AssaultGun | ASSAULT_GUN | Assault Guns | Assault Gun |
| ROCKTLCH | RocketLauncher | ROCKET_LAUNCHER | Rocket Launcher | Rocket Launcher |
| MISSLLCH | MissileCrawler | MISSILE_CRAWLER | Missile Crawlers | Missile Crawler |
| SP_FLAK | MobileAntiAir | MOBILE_ANTI_AIRCRAFT | Mobile Anti-Air | Mobile Anti-Air |
| MINELAYR | MineLayer | MINE_LAYER | Mine Layers | Mine Layer |
| SURVEYOR | Surveyor | SURVEYOR | Surveyors | Surveyor |
| SCANNER | Scanner | SCANNER | Mobile Scanners | Scanner |
| SPLYTRCK | SupplyTruck | SUPPLY_TRUCK | Supply Trucks | Supply Truck |
| GOLDTRCK | GoldTruck | GOLD_TRUCK | Gold Truck | Gold Truck |
| ENGINEER | Engineer | ENGINEER | Engineers | Engineer |
| BULLDOZR | Bulldozer | BULLDOZER | Bulldozers | Bulldozer |
| REPAIR | RepairUnit | REPAIR_UNIT | Repair Units | Repair Unit |
| FUELTRCK | FuelTruck | FUEL_TRUCK | Fuel Truck | Fuel Truck |
| CLNTRANS | ArmouredPersonnelCarrier | PERSONNEL_CARRIER | Armoured Personnel Carriers | Armoured Personnel Carrier |
| COMMANDO | Infiltrator | INFILTRATOR | Infiltrator | Infiltrator |
| INFANTRY | Infantry | INFANTRY | Infantry | Infantry |
| FASTBOAT | Escort | ESCORT | Escorts | Escort |
| CORVETTE | Corvette | CORVETTE | Corvette | Corvette |
| BATTLSHP | Gunboat | GUNBOAT | Gunboats | Gunboat |
| SUBMARNE | Submarine | SUBMARINE | Submarines | Submarine |
| SEATRANS | SeaTransport | SEA_TRANSPORT | Supply Boats | Sea Transport |
| MSSLBOAT | MissileCruiser | MISSILE_CRUISER | Missile Cruisers | Missile Cruiser |
| SEAMNLYR | SeaMineLayer | SEA_MINE_LAYER | Sea Mine Layer | Sea Mine Layer |
| CARGOSHP | CargoShip | CARGO_SHIP | Supply Boats | Cargo Ship |
| FIGHTER | Fighter | FIGHTER | Fighters | Fighter |
| BOMBER | Bomber | GROUND_ATTACK_PLANE | Ground Attack Planes | Bomber |
| AIRTRANS | AirTransport | AIR_TRANSPORT | Air Transports | Air Transport |
| AWAC | Awac | AWAC | AWAC | AWAC |
| JUGGRNT | Juggernaut | ALIEN_GUNBOAT | — | Juggernaut |
| ALNTANK | AlienTank | ALIEN_TANK | — | Alien Tank |
| ALNASGUN | AlienAssaultGun | ALIEN_ASSAULT_GUN | — | Alien Assault Gun |
| ALNPLANE | AlienPlane | ALIEN_PLANE | — | Alien Plane |

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
- `BOMBER`: C++/мануал — «Ground Attack Plane»; решение владельца — «Bomber».
- `SEATRANS`: мануал — «Supply Boats»; решение владельца — «Sea Transport».
- `CARGOSHP`: мануал — «Supply Boats» (совпадает с `SEATRANS` в списке верфи);
  уточнить корректный User-Friendly Name.
