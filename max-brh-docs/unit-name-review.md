# Финальное ревью имён юнитов

Сверка «Rust (код) / C++ / мануал / User-Friendly Name». C++-идентификаторы — из
doc-комментариев `UnitType` (`/// TOKEN / C++_NAME`); мануал — `MANUAL.PDF`;
описание (en) и тип — из оригинала Interplay (`SC_UNITS` + `language.json`,
классификация — `object-classifier.md`). Эффекты/декорации/alien мануал не
описывает (в столбце «Мануал» — «—»).

> **Указание разработчику.**
> - В **коде** используется имя из колонки **Rust** (идентификатор `UnitType`).
> - В **интерфейсе игры** отображается имя из колонки **User-Friendly Name**
>   (английская строка, как в оригинальном мануале/игре).
> - Переименования текущего кода сведены в раздел «Переименования» ниже.

## Юниты и здания

| № | Токен | Rust (код) | C++ | Мануал | Описание (en) | Тип | User-Friendly Name |
|---|---|---|---|---|---|---|---|
| 1 | COMMTWR | GoldRefinery | UNIT_TYPE_GOLD_REFINERY | Gold Refinery | Refinery for converting gold into credits.  Credits are required to purchase unit improvements.  To run, a gold refinery needs a source of power and a source of gold ore.  A mining station can produce gold ore if one of its four squares covers a square with underground gold. | Здание | Gold Refinery |
| 2 | POWERSTN | PowerStation | POWER_STATION | Power Stations | A power station consumes six fuel each turn, and provides enough power for six factories or mining stations. | Здание | Power Station |
| 3 | POWGEN | PowerGenerator | POWER_GENERATOR | Power Generators | A power generator consumes two fuel each turn, and provides enough power for one mining station or factory.  The power generator must be connected to the fuel source (usually a mining station) and the building that needs the power. | Стационарный | Power Generator |
| 4 | BARRACKS | Barracks | BARRACKS | Barracks | A barracks holds infiltrators and infantry units.  Inside the barracks is a machine shop for repairing and improving powered suits, and for manufacturing ammunition.  The barracks needs raw materials to repair, upgrade, or resupply, so it should be connected to a storage unit or a mining station. | Здание | Barracks |
| 5 | SHIELDGN | MonopoleMine | ALIEN_BUILDING_1 | — | Specialized mining station for extracting magnetic monopoles. | Здание | Monopole Mine |
| 6 | RADAR | Radar | RADAR | Radar | Stationary, long-range radar.  Longer-range units like missile launchers and artillery cannot fire on what they cannot see, so it's important to have a scanner or radar nearby. | Стационарный | Radar |
| 7 | ADUMP | MaterialStorage | STORAGE_UNIT | Material Storage | Holds raw materials.  Mining stations produce raw materials every turn.  To save extra raw materials, a mining station must be connected to a storage unit. | Стационарный | Material Storage |
| 8 | FDUMP | FuelStorage | FUEL_TANK | Fuel Storage | Holds fuel reserves.  Mining stations produce fuel every turn.  To save extra fuel, a mining station must be connected to a fuel tank. | Стационарный | Fuel Storage |
| 9 | GOLDSM | GoldVault | GOLD_VAULT | Gold Vault | Holds unrefined gold ore.  Mining stations produce gold every turn, if they have gold ore underneath them.  To store the ore, a mining station must be connected to a gold vault. | Стационарный | Gold Vault |
| 10 | DEPOT | Depot | DEPOT | Depot | A depot can repair damage, manufacture ammunition, and refit ground units with the newest technology.  All of these operations require raw materials, so depots should be connected to stored materials. | Здание | Depot |
| 11 | HANGAR | Hangar | HANGAR | Hangar | A hangar can repair damage, manufacture ammunition, and refit planes with the newest technology.  All of these operations require raw materials, so hangars should be connected to stored materials. | Здание | Hangar |
| 12 | DOCK | Dock | DOCK | Dock | A dock can repair damage, manufacture ammunition, and refit ships with the newest technology.  All of these operations require raw materials, so docks should be connected to stored materials. | Здание | Dock |
| 13 | CNCT_4W | Connector | CONNECTOR | Connectors | Buildings must be connected to share power, fuel, raw materials, and gold.  You can connect buildings by placing them next to each other, or by building connectors from one building to the other. | Стационарный | Connector |
| 14 | ROAD | Road | ROAD | Road | Units move twice as fast over road squares. | Стационарный | Road |
| 15 | LANDPAD | LandingPad | LANDING_PAD | Landing Pad | Landing facility for planes.  Does not repair, resupply, or refuel planes.  To resupply a plane on a landing pad, use a nearby truck or storage unit. | Стационарный | Landing Pad |
| 16 | SHIPYARD | Shipyard | SHIPYARD | Shipyard | Shipyards manufacture ships, such as submarines, sea transports, and missile cruisers.  A shipyard requires three raw materials a turn to operate, and power from a power generator or power station. | Здание | Shipyard |
| 17 | LIGHTPLT | LightVehiclePlant | LIGHT_VEHICLE_PLANT | Light Vehicle Plant | Builds light vehicles: engineers, trucks, mobile repair units, scouts, and mobile anti aircraft units.  A light vehicle plant requires three raw materials a turn to operate, and power from a power generator or power station. | Здание | Light Vehicle Plant |
| 18 | LANDPLT | HeavyVehiclePlant | HEAVY_VEHICLE_PLANT | Heavy Vehicle Plant | Builds constructors, mobile scanners, and the heavy fighting units: tanks, assault guns, rocket launchers, and missile crawlers.  A heavy factory needs three raw materials a turn to operate, and power from a power generator or power station. | Здание | Heavy Vehicle Plant |
| 19 | SUPRTPLT | SupportPlant | ALIEN_BUILDING_2 | — | — | Здание | Support Plant |
| 20 | AIRPLT | AirUnitsPlant | AIR_UNITS_PLANT | Air Units Plant | Builds fighters, ground attack planes, and air transports.  A heavy factory needs three raw materials a turn to operate, and power from a power generator or power station. | Здание | Air Units Plant |
| 21 | HABITAT | Habitat | HABITAT | Habitat | Habitats house colonists.  Each habitat houses enough colonists to operate three research centers, training halls, or eco-spheres.  The colonist buildings must be connected to the habitat before they can operate. | Здание | Habitat |
| 22 | RESEARCH | ResearchCentre | RESEARCH_CENTER | Research Centre | Research centers investigate ways to improve your units.  Unlike upgrades purchased with refined gold, research takes time, and the improvements are small.  However, research affects all of your units instead of just one type, and doesn't require a source of gold ore.  A research center requires colonists from a Habitat to operate, and power. | Здание | Research Centre |
| 23 | GREENHSE | EcoSphere | ECOSPHERE | Eco-Sphere | Domed building containing an earthlike environment.  Your ultimate goal is to provide as many of these for your colonists as possible.  Every turn an ecosphere operates, it increases your colony rating.  If its destroyed, you lose all of the improvements the ecosphere made to your rating.  Ecospheres require colonists from a Habitat and power to operate. | Здание | Eco-Sphere |
| 24 | RECCENTR | RecreationCenter | ALIEN_BUILDING_3 | — | — | Здание | Recreation Center |
| 25 | TRAINHAL | TrainingHall | TRAINING_HALL | Training Hall | Trains infiltrators who can disable or capture enemy units, and infantry to guard against infiltrators.  Only infantry and other infiltrators can detect infiltrators.  A training hall requires colonists from a Habitat and power to operate. | Здание | Training Hall |
| 26 | WTRPLTFM | WaterPlatform | WATER_PLATFORM | Water Platform | Provides a surface over water on which to construct other buildings.  Engineers can build water platforms over shore as well as open water.  Water platforms are particularly important on island maps, or when a valuable material deposit is under water. | Стационарный | Water Platform |
| 27 | GUNTURRT | GunTurret | GUN_TURRET | Gun Turret | Heavily armored stationary gun turret.  Gun turrets have good firepower, heavy armor, and moderate range.  Because they take little time to build, they are a good choice when you need defense in a hurry. | Здание | Gun Turret |
| 28 | ANTIAIR | AntiAir | ANTI_AIRCRAFT | Anti-Air | Low caliber, high speed anti aircraft cannon.   Anti aircraft make short work of planes, but cannot attack ground units. | Здание | Anti-Air |
| 29 | ARTYTRRT | Artillery | ARTILLERY | Artillery | High powered stationary gun.  Artillery is a compromise between the cheap, rugged gun turret and the expensive, long-range missile turret. | Здание | Artillery |
| 30 | ANTIMSSL | MissileLauncher | MISSILE_LAUNCHER | Missile Launcher | Long-range missile launcher on a fixed mount. Excellent range and firepower, but only slightly more armor than the mobile version.  Best surrounded by gun turrets and anti aircraft. | Здание | Missile Launcher |
| 31 | BLOCK | ConcreteBlock | CONCRETE_BLOCK | Concrete Block | Hardened, armored block for blocking enemy movement. | Стационарный | Concrete Block |
| 32 | BRIDGE | Bridge | BRIDGE | Bridge | Pontoon bridge for crossing water.  Bridges, unlike water platforms, do not block the movement of ships. | Стационарный | Bridge |
| 33 | MININGST | MiningStation | MINING_STATION | Mining Stations | Extracts raw materials, fuel, and gold from the ground every turn.  Buildings must be connected to a mine to use the resources a mine produces.  If there are no storage buildings, fuel tanks, or gold vaults to store extra material, fuel, or gold, it will be lost. | Здание | Mining Station |
| 34 | LANDMINE | LandMine | LAND_MINE | — | Small, hard to detect explosive device.  Only surveyors, minelayers, and infiltrators can spot mines.  Once spotted, almost any damage will destroy it. | Стационарный | Land Mine |
| 35 | SEAMINE | SeaMine | SEA_MINE | — | Small, hard to detect explosive device.  Only surveyors, minelayers, and infiltrators can spot mines.  Once spotted, almost any damage will destroy it. | Стационарный | Sea Mine |
| 36 | CONSTRCT | Constructor | CONSTRUCTOR | Constructors | Large construction vehicle for creating large buildings, such as mines and factories.  A constructor consumes two material per turn while operating. | Амфибия | Constructor |
| 37 | SCOUT | Scout | SCOUT | Scouts | High speed scouting vehicle for exploration and spotting enemy units.  Can cross water without a bridge, but is faster on land.  Very fast and good scan range, but much weaker than most combat units. | Амфибия | Scout |
| 38 | TANK | Tank | TANK | Tanks | Heavily armored fighting vehicle.  Best used in the front line to prevent enemy units from reaching lightly-armored support units such as assault guns and rocket launchers. | Наземный | Tank |
| 39 | ARTILLRY | AssaultGun | ASSAULT_GUN | Assault Guns | Lightly armored vehicle with a long range gun firing high-explosive shells.  Fast and effective on the attack, but fragile. | Наземный | Assault Gun |
| 40 | ROCKTLCH | RocketLauncher | ROCKET_LAUNCHER | Rocket Launcher | Lightly armored vehicle firing medium-range rockets which affect all units within 2 squares of the target.  Most effective against groups of enemy units. | Наземный | Rocket Launcher |
| 41 | MISSLLCH | MissileCrawler | MISSILE_CRAWLER | Missile Crawlers | Missile launcher on a lightly armored chassis.  This mobile launcher is slightly less sturdy than the stationary version, and requires more material to build.  However, the ability to move makes it much more flexible. | Наземный | Missile Crawler |
| 42 | SP_FLAK | MobileAntiAir | MOBILE_ANTI_AIRCRAFT | Mobile Anti-Air | Light, fast vehicle with a rapid fire cannon for shooting down enemy planes.  Mobile anti aircraft are generally defensive units, because they aren't as fast as planes, and can't move while firing.  Use them to protect an area once your fighters have chased away enemy ground attack planes. | Наземный | Mobile Anti-Air |
| 43 | MINELAYR | MineLayer | MINE_LAYER | Mine Layers | Minelayers are specialized construction vehicles which place, detect, and remove mines.  Minelayers convert onboard raw materials into mines.  They can also remove those mines later and convert them back into materials.  They cannot remove enemy minefields - those must be exploded with gunfire and rockets. | Наземный | Mine Layer |
| 44 | SURVEYOR | Surveyor | SURVEYOR | Surveyors | Light, amphibious vehicle with sophisticated sensors for detecting underground minerals and enemy mines.  Unlike most amphibious units, surveyors are just as fast on water as on land. | Амфибия | Surveyor |
| 45 | SCANNER | Scanner | SCANNER | Mobile Scanners | Mobile radar platform.  Slower and more expensive than a scout, but with a better scan range.  A scanner is an excellent way to spot targets for very long ranged units like missile crawlers. | Наземный | Scanner |
| 46 | SPLYTRCK | SupplyTruck | SUPPLY_TRUCK | Supply Trucks | Truck for hauling raw material.  Useful for resupplying engineers and constructors, and for supplying combat units with new ammunition. | Наземный | Supply Truck |
| 47 | GOLDTRCK | GoldTruck | GOLD_TRUCK | Gold Truck | Unit for moving gold ore from one location to another. | Наземный | Gold Truck |
| 48 | ENGINEER | Engineer | ENGINEER | Engineers | Small construction vehicle for creating small buildings and stationary weapons. | Амфибия | Engineer |
| 49 | BULLDOZR | Bulldozer | BULLDOZER | Bulldozers | Vehicle for clearing rubble and demolishing buildings. Bulldozers can scavenge material from debris. | Наземный | Bulldozer |
| 50 | REPAIR | RepairUnit | REPAIR_UNIT | Repair Units | Light vehicle which can rapidly repair ground units and buildings.  Repairs require materials.  The more extensive the damage, the more materials the repair requires.  Depots, docks, hangars, and barracks can also repair units. | Наземный | Repair Unit |
| 51 | FUELTRCK | FuelTruck | FUEL_TRUCK | Fuel Truck | Truck for moving fuel. | Наземный | Fuel Truck |
| 52 | CLNTRANS | ArmouredPersonnelCarrier | PERSONNEL_CARRIER | Armoured Personnel Carriers | Fast armored to transport infantry and infiltrators.  The APC can move slowly underwater as well as on land.  Only enemy corvettes can detect an APC moving underwater. | Амфибия | Armoured Personnel Carrier |
| 53 | COMMANDO | Infiltrator | INFILTRATOR | Infiltrator | Commando trained in the arts of stealth and electronic warfare.  Infiltrators can disable or capture enemy units. Infiltrators are normally invisible until they make a mistake trying to capture enemy units, or until spotted by infantry or infiltrators. | Наземный | Infiltrator |
| 54 | INFANTRY | Infantry | INFANTRY | Infantry | Human soldiers equipped with light anti-tank shoulder weapons.  Infantry can spot enemy infiltrators. | Наземный | Infantry |
| 55 | FASTBOAT | Escort | ESCORT | Escorts | High speed boat with good radar and rapid fire anti aircraft cannon.  Escorts are the eyes of a fleet, and a fleet's protection from planes. | Морской | Escort |
| 56 | CORVETTE | Corvette | CORVETTE | Corvette | High speed boat with torpedo tubes and sonar.  Corvettes can attack any sea unit, but their real strength is their ability to spot and attack submarines. | Морской | Corvette |
| 57 | BATTLSHP | Gunboat | GUNBOAT | Gunboats | Heavily armored ship with a high caliber, medium range cannon.  Excellent at destroying other ships and bombarding shore targets. | Морской | Gunboat |
| 58 | SUBMARNE | Submarine | SUBMARINE | Submarines | Lurking beneath the surface, submarines must sneak close to the enemy to fire their powerful torpedoes.  Submarines do not carry active sonar, which might reveal their positions.  Only corvettes can spot a submarine before it fires.  Only submarines, corvettes, and ground attack planes can attack them. | Морской | Submarine |
| 59 | SEATRANS | SeaTransport | SEA_TRANSPORT | Supply Boats | Heavily armored ship with space to carry up to six land units.  Sea transports are much slower than air transports, but also much more likely to survive an enemy attack. | Морской | Sea Transport |
| 60 | MSSLBOAT | MissileCruiser | MISSILE_CRUISER | Missile Cruisers | Ship mounted missile launcher.  Excellent range, high attack strength, and medium armor makes this a powerful unit. | Морской | Missile Cruiser |
| 61 | SEAMNLYR | SeaMineLayer | SEA_MINE_LAYER | Sea Mine Layer | Seaborne version of the minelayer.  Like its land based counterpart, the sea minelayer can manufacture mines, detect them, and convert them back into raw material. | Морской | Sea Mine Layer |
| 62 | CARGOSHP | CargoShip | CARGO_SHIP | Supply Boats | Ships for hauling raw material.  Useful for resupplying engineers and sea combat units with new ammunition. | Морской | Cargo Ship |
| 63 | FIGHTER | Fighter | FIGHTER | Fighters | Fast but fragile aircraft carrying a limited number of air-to-air missiles.  Fighters are the best way to drive enemy planes out of an area, and to defend a moving force.  Ground based anti aircraft is a better choice for defending a fixed area. | Воздушный | Fighter |
| 64 | BOMBER | Bomber | GROUND_ATTACK_PLANE | Ground Attack Planes | A heavier plane carrying a full load of air to ground missiles.  Ground Attack Planes are expensive, but the advantages are worth the cost.  Their great speed lets them reach any trouble spot quickly, and once they arrive only fighters and anti aircraft can defend against them. | Воздушный | Ground Attack Plane |
| 65 | AIRTRANS | AirTransport | AIR_TRANSPORT | Air Transports | Heavy aircraft capable of holding up to three ground units. | Воздушный | Air Transport |
| 66 | AWAC | Awac | AWAC | AWAC | Airborne Warning And Control plane.  Basically a flying radar dish, an AWAC is essential to spot enemy anti aircraft before your planes fly into firing range. | Воздушный | AWAC |
| 67 | JUGGRNT | Juggernaut | ALIEN_GUNBOAT | — | Heavily armored and armed warship of alien design.  Like the human gunboat, the alien gunboat excels at destroying other ships and land targets near the shore.  All alien units repair themselves, and improve their abilities with experience. | Морской | Juggernaut |
| 68 | ALNTANK | AlienTank | ALIEN_TANK | — | Heavily armored tank of alien design.  Alien tanks are best used at the front of an attack to soak up enemy fire.  All alien units repair themselves, and improve their abilities with experience. | Наземный | Alien Tank |
| 69 | ALNASGUN | AlienAssaultGun | ALIEN_ASSAULT_GUN | — | Powerful plasma weapon mounted on a fast, medium-armored chassis.  While not as fragile as human assault guns, the alien assault gun should still avoid enemy fire whenever possible.  All alien units repair themselves, and improve their abilities with experience. | Наземный | Alien Assault Gun |
| 70 | ALNPLANE | AlienPlane | ALIEN_PLANE | — | Alien fighter-bomber.  While the alien attack plane is primarily designed to attack ground targets, it can also fire its missiles at other planes.  Though powerful, the alien attack plane is still vulnerable to anti aircraft fire.  Like other alien units, alien attack planes repair themselves, and improve their abilities with experience. | Воздушный | Alien Plane |

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
| ShieldGenerator | MonopoleMine |

## Открытые вопросы (требуют решения владельца)

— (все вопросы решены)

## Решения владельца

- `SHIELDGN` (C++ `ALIEN_BUILDING_1`): Rust-имя `MonopoleMine`, User-Friendly Name —
  «Monopole Mine» (рудимент Interplay; токен/ассет `SHLDGEN.FLC` — пережиток
  аббревиатуры «Shield GeNerator», а не «Shield Generator»).
- `SUPRTPLT` (C++ `ALIEN_BUILDING_2`): Rust-имя `SupportPlant`, User-Friendly Name —
  «Support Plant» (рудимент Interplay, намек на недоделанный Life Support;
  каноничного имени в оригинале нет).
- `RECCENTR` (C++ `ALIEN_BUILDING_3`): Rust-имя `RecreationCenter`, User-Friendly Name —
  «Recreation Center» (рудимент Interplay).
- Все три — «alien derelict buildings»: не строятся игроком, не в мануале;
  спавнятся только опцией «Alien Derelicts» командой `PLAYER_TEAM_ALIEN`.
- `BOMBER`: Rust-имя `Bomber`, User-Friendly Name — «Ground Attack Plane».
- `SEATRANS`: User-Friendly Name — «Sea Transport» (морской транспорт).
- `CARGOSHP`: User-Friendly Name — «Cargo Ship» (грузовое судно).
- Авторитет имён: `unit-name-review.md` — канон; `domain-map.md` приведён в
  соответствие.
