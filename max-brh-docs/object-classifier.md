# Системный классификатор объектов игры

Каноническая таксономия игровых объектов (DSL). Ведётся аналитиком; соответствует
`domain-map.md` (таблица «домен-термин → Rust-имя → C++-идентификатор»).

## Ключевые критерии (по данным)

- **Боевой/небоевой** — атрибут **`attack`** (наносимый урон): `attack > 0` —
  боевой; `attack = 0` — небоевой.
- **Мобильный/стационарный** — атрибут **`speed`**: `speed > 0` — мобильный;
  `speed = 0` — стационарный.

Атрибуты берутся из базовых значений типа (`BASE_UNIT_VALUES`, C++
`SC_ATTRI`/`ATTRIB_ATTACK`/`ATTRIB_SPEED`).

## Юниты

```
Юнит (Unit)
├── Боевой (Combat) — attack > 0
│   ├── Боевое здание (стационарные пушки)
│   │   └── GUNTURRT (пушка), ARTYTRRT (артиллерийская), ANTIAIR (зенитка), ANTIMSSL (ракетомёт)
│   ├── Боевой стационарный (мины)
│   │   └── LANDMINE (наземная), SEAMINE (морская)
│   └── Боевой мобильный
│       ├── Наземный: TANK, ARTILLRY, ROCKTLCH, MISSLLCH, SP_FLAK
│       ├── Воздушный: FIGHTER, BOMBER
│       ├── Морской: CORVETTE, FASTBOAT, BATTLSHP, SUBMARNE, MSSLBOAT
│       ├── Парящий (HOVERING): SCOUT
│       ├── Пехота: INFANTRY, COMMANDO
│       └── Alien (боевые, несколько внутренних категорий): JUGGRNT, ALNTANK, ALNASGUN, ALNPLANE
└── Небоевой (Non-combat) — attack = 0
    ├── Небоевое здание
    │   ├── Производящее: BARRACKS, TRAINHAL, LIGHTPLT, LANDPLT, AIRPLT, SHIPYARD, SUPRTPLT
    │   ├── Непроизводящее: COMMTWR, POWERSTN, POWGEN, SHIELDGN, RADAR, ADUMP, FDUMP, GOLDSM,
    │   │                   DEPOT, HANGAR, DOCK, HABITAT, RESEARCH, GREENHSE, RECCENTR, MININGST,
    │   │                   LANDPAD, WTRPLTFM, ROAD, BRIDGE, BLOCK
    │   └── Коннектор (инфраструктура комплекса): CNCT_4W
    └── Небоевой мобильный
        ├── Строитель (наземный): CONSTRCT, ENGINEER, BULLDOZR
        ├── Разведчик/наблюдение: SURVEYOR, SCANNER (наземные), AWAC (воздушный)
        ├── Снабжение (наземный): SPLYTRCK, FUELTRCK, GOLDTRCK, REPAIR
        ├── Миноукладчик: MINELAYR (наземный), SEAMNLYR (морской)
        └── Транспорт: AIRTRANS (воздушный), CLNTRANS (наземный), SEATRANS, CARGOSHP (морской)
```

### Оси классификации юнитов

| Ось | Значения | Rust-имя (маркер/признак) | C++ |
|---|---|---|---|
| Боевая роль (верхний уровень) | Боевой / Небоевой | `Combat` / `NonCombat` | `attack > 0` / `attack = 0` |
| Форма | Здание / Мобильный / Стационарный-прочий | `Building` / `Mobile` / `Stationary` | `speed = 0` / `speed > 0` |
| Среда мобильного | Воздушный / Наземный / Морской / Парящий | `Air` / `Ground` / `Sea` / `Hovering` | `MOBILE_AIR_UNIT` / `MOBILE_LAND_UNIT` / `MOBILE_SEA_UNIT` / `HOVERING` |
| Производство (небоевые здания) | Производящее / Непроизводящее | `Producing` / `NonProducing` | — |

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

- Эффекты и декорации логически не входят в классификатор юнитов (смешение —
  нарушение слоёв). Текущее совмещение их с `UnitType` (93 типа) — технический
  долг, устраняется по правилу 2 программиста.
- Сюжетные декорации (`MASTER`, `HARVSTER`, `WALDO`) в игровой классификатор не
  входят; фиксируются как специальные объекты сюжетных миссий.
- Боевой/небоевой определяется **по данным** (`attack`), а не по роли; роль
  (разведчик/транспорт/снабжение) — вторичная ось.
- Конкретное соответствие «тип → класс/ось» ведётся в `domain-map.md`.
