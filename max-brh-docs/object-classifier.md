# Системный классификатор объектов игры

Каноническая таксономия игровых объектов (DSL). Ведётся аналитиком; соответствует
`domain-map.md` (таблица «домен-термин → Rust-имя → C++-идентификатор»).

## Ключевые критерии (по данным)

- **Боевой/небоевой** — атрибут **`attack`** (`ATTRIB_ATTACK`): `attack > 0` —
  боевой; `attack = 0` — небоевой (таблица `BASE_UNIT_VALUES`, C++ `SC_ATTRI`).
- **Мобильный/стационарный** — флаг **`STATIONARY` (0x200)**: установлен —
  неподвижный (`speed = 0`), снят — мобильный (`speed > 0`). Атрибут `speed` —
  следствие, а не маркер.
- **Здание/стационарный-прочий** — флаг **`BUILDING` (0x10)** = «2×2-структура»:
  `Building = STATIONARY ∧ BUILDING`; `Stationary = STATIONARY ∧ ¬BUILDING`
  (`BUILDING ⟹ STATIONARY`). Семантическое исключение (решение владельца):
  турели (`GUNTURRT`/`ARTYTRRT`/`ANTIAIR`/`ANTIMSSL`) — **здания** по смыслу,
  хотя по флагу это 1×1 `STATIONARY` без `BUILDING`; мины — **стационарные
  юниты** (не здания).
- **Среда мобильного** — флаги `MOBILE_AIR_UNIT` (0x40) / `MOBILE_SEA_UNIT`
  (0x80) / `MOBILE_LAND_UNIT` (0x100). **`HOVERING` (0x10000) — не среда**, а
  runtime-состояние полёта воздуха (`UnitInfo::Take`/`Land`), в `SC_UNITS` не
  встречается.

Атрибуты — из `BASE_UNIT_VALUES` (C++ `SC_ATTRI`); флаги — из `SC_UNITS`
(`UNIT_TYPE_PROPERTIES`, `access.rs`).

## Юниты

```
Юнит (Unit)
├── Боевой (Combat) — attack > 0
│   ├── Боевое здание (стационарные пушки)
│   │   └── GUNTURRT (пушка), ARTYTRRT (артиллерийская), ANTIAIR (зенитка), ANTIMSSL (ракетомёт)
│   ├── Боевой стационарный (мины — неподвижные юниты)
│   │   └── LANDMINE (наземная), SEAMINE (морская)
│   └── Боевой мобильный
│       ├── Наземный: TANK, ARTILLRY, ROCKTLCH, MISSLLCH, SP_FLAK
│       ├── Амфибийный (наземно-морской): SCOUT
│       ├── Воздушный: FIGHTER, BOMBER
│       ├── Морской: CORVETTE, FASTBOAT, BATTLSHP, SUBMARNE, MSSLBOAT
│       ├── Пехота: INFANTRY, COMMANDO
│       └── Alien (боевые): JUGGRNT (морской), ALNTANK/ALNASGUN (наземные), ALNPLANE (воздушный)
└── Небоевой (Non-combat) — attack = 0
    ├── Небоевое здание
    │   ├── Производящее (заводы юнитов): TRAINHAL, LIGHTPLT, LANDPLT, AIRPLT, SHIPYARD
    │   ├── Производящее ресурсы/рабочих: POWERSTN, POWGEN, COMMTWR, HABITAT,
    │   │                                GREENHSE, RESEARCH, MININGST
    │   ├── Непроизводящее: RADAR, ADUMP, FDUMP, GOLDSM, DEPOT, HANGAR,
    │   │                   DOCK, BARRACKS, LANDPAD, WTRPLTFM,
    │   │                   ROAD, BRIDGE, BLOCK
    │   └── Коннектор (инфраструктура комплекса): CNCT_4W
    └── Alien derelict (рудименты Interplay, не строятся игроком)
        └── SHIELDGN (Monopole Mine), SUPRTPLT (Support Plant), RECCENTR (Recreation Center)
    └── Небоевой мобильный
        ├── Строитель: CONSTRCT, ENGINEER (амфибийные), BULLDOZR, MINELAYR (наземные), SEAMNLYR (морской)
        ├── Разведчик/наблюдение: SURVEYOR (амфибийный), SCANNER (наземный), AWAC (воздушный)
        ├── Снабжение (наземный): SPLYTRCK, FUELTRCK, GOLDTRCK, REPAIR
        └── Транспорт: AIRTRANS (воздушный), CLNTRANS (амфибийный), SEATRANS, CARGOSHP (морской)
```

### Оси классификации юнитов

Классификацию несёт `enum` (единственный источник истины); поведение — поведенческие
типажи (решение владельца, вариант C).

| Ось | Значения | Rust (enum) | C++ |
|---|---|---|---|
| Боевая роль | Боевой / Небоевой | `CombatRole::{Combat, NonCombat}` | `attack > 0` / `attack = 0` |
| Форма | Здание / Мобильный / Стационарный-прочий | `Mobility::{Building, Mobile, Stationary}` | `BUILDING(0x10)`† / `¬STATIONARY` / `STATIONARY(0x200)∧¬BUILDING` |
| Футпринт (размер на карте) | 1×1 / 2×2 | `Footprint::{Single, Quad}` | `BUILDING(0x10)` |
| Среда мобильного | Воздушный / Амфибийный / Наземный / Морской | `Environment::{Air, Amphibious, Ground, Sea}` | `MOBILE_AIR_UNIT` / `SEA\|LAND` / `MOBILE_LAND_UNIT` / `MOBILE_SEA_UNIT` |
| Производственный | Производственный / Непроизводственный | `is_producing()` | `Cargo_GetNetProduction` (12 типов) |
| Cargo (тип) | Без / Ресурсный / Юнитный | `CargoKind::{None, Resource, Unit}` | `cargo_type` (`CARGO_TYPE_*`) |

† Турели — семантически здания (`Mobility::Building`), но `Footprint::Single`
(флаг `BUILDING` не установлен). Инвариант: `Footprint::Quad ⟹ Mobility::Building`
(обратное неверно). `Footprint` — лайаут, `Mobility::Building` — семантика.

Стабильные теги сериализации `Footprint` (не полагаться на порядок вариантов):
`Single = 0`, `Quad = 1`.

## Производственные юниты

Производственный юнит — юнит с **потреблением** и **производством** (ресурсы,
рабочие, юниты, кредиты). Отдельной оси «энергия» нет: энергия — один из видов
потребления. Источник — `Cargo_GetNetProduction` (ровно 12 типов): 5 заводов
юнитов + 6 ресурсных зданий + шахта. Свойства производственного юнита:

- **Потребление** — что нужно для работы: энергия, сырьё, топливо, золото, рабочие.
- **Производство** — что выдаёт: сырьё, топливо, золото, рабочие, юниты, кредиты.
- **Режимы** (несколько):
  1. **Power On/Off** — базовый режим (включён/выключен);
  2. **Аллокация шахты** — перераспределение добычи (сырьё/топливо/золото) в комплекс;
  3. **Скорость завода** — множитель: ускорение производства за счёт увеличения
     потребления сырья за ход.

Примеры:

| Юнит | Потребляет | Производит |
|---|---|---|
| Шахта (`MININGST`) | энергия | сырьё / топливо / золото (по аллокации) |
| Очиститель золота (`COMMTWR`) | энергия + золотая руда | очищенное золото (кредиты) |
| Дом (`HABITAT`) | — (без энергии) | 3 рабочих |
| Экосфера (`GREENHSE`) | 1 рабочий + 1 энергия | 1 колонист/ход |

Подробные свойства по каждому юниту — [`unit-properties.md`](unit-properties.md).

## Не-юниты

```
Не-юнит
├── Эффект (Effect)
│   ├── Снаряд (Projectile): ROCKET, TORPEDO, ALNMISSL, ALNTBALL, ALNABALL
│   ├── Взрыв (Explosion): LNDEXPLD, AIREXPLD, SEAEXPLD, BLDEXPLD, HITEXPLD
│   ├── Обломок (Rubble): LRGRUBLE, SMLRUBLE
│   └── След (Trail): RKTSMOKE, TRPBUBLE
├── Декорация/стройплощадка (Decoration): LRGTAPE, SMLTAPE, LRGSLAB, SMLSLAB, LRGCONES, SMLCONES
└── Сюжетная декорация (удалённые/скрытые): MASTER, HARVSTER, WALDO
```

## Примечания

- **`HOVERING` (0x10000) — runtime-состояние, а не класс типа.** Отсутствует в
  `SC_UNITS`; выставляется `UnitInfo::Take()` и снимается `UnitInfo::Land()`
  только у воздушных юнитов (`MOBILE_AIR_UNIT`). Не используется для
  классификации.
- **Амфибийные юниты** (`MOBILE_SEA_UNIT | MOBILE_LAND_UNIT`): `SCOUT`, `CONSTRCT`,
  `ENGINEER`, `SURVEYOR`, `CLNTRANS` — ходят по суше и воде
  (`land_type = LAND|WATER|COAST`).
- **`BUILDING` (0x10) = 2×2-структура**, а не «любое здание»: турели, мины,
  `POWGEN`, `RADAR`, хранилища, коннектор — 1×1 `STATIONARY` без `BUILDING`.
  Семантически турели — здания, мины — стационарные юниты (решение владельца).
- **Cargo имеет тип, и он взаимоисключающий:** определяется `cargo_type`
  (`RAW`/`FUEL`/`GOLD` — ресурсный; `LAND`/`SEA`/`AIR` — юнитный), а не только
  `storage`. Оба типа у одного вместилища не встречаются.
- **Миноукладчики** (`MINELAYR`, `SEAMNLYR`) — по природе строители: строят мины
  за материал из своего хранилища (ресурсный cargo).
- **`BARRACKS` и `SUPRTPLT` — не производящие** (`Cargo_GetNetProduction` их не
  содержит): `BARRACKS` — ремонт/апгрейд пехоты, `SUPRTPLT` — alien-здание.
- Эффекты и декорации логически не входят в классификатор юнитов (смешение —
  нарушение слоёв). Текущее совмещение их с `UnitType` (93 типа) — технический
  долг, устраняется по правилу 2 программиста.
- Сюжетные декорации (`MASTER`, `HARVSTER`, `WALDO`) в игровой классификатор не
  входят; фиксируются как специальные объекты сюжетных миссий.
- Конкретное соответствие «тип → класс/ось» ведётся в `domain-map.md`.
