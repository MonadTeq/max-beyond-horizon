# Системный классификатор объектов игры

Каноническая таксономия игровых объектов (DSL). Ведётся аналитиком; соответствует
`domain-map.md` (таблица «домен-термин → Rust-имя → C++-идентификатор»).

## Юниты

```
Юнит (Unit)
├── Боевой (Combat)
│   ├── Боевое здание (стационарные пушки)
│   │   └── GUNTURRT (пушка), ARTYTRRT (артиллерийская), ANTIAIR (зенитка), ANTIMSSL (ракетомёт)
│   ├── Боевой стационарный (мины)
│   │   └── LANDMINE (наземная), SEAMINE (морская)
│   └── Боевой мобильный
│       ├── Наземный: TANK, ARTILLRY, ROCKTLCH, MISSLLCH, SP_FLAK
│       ├── Воздушный: FIGHTER, BOMBER, AWAC
│       ├── Морской: CORVETTE, FASTBOAT, BATTLSHP, SUBMARNE, MSSLBOAT
│       ├── Пехота: INFANTRY, COMMANDO
│       └── Alien (боевые, несколько внутренних категорий): JUGGRNT, ALNTANK, ALNASGUN, ALNPLANE
└── Небоевой (Non-combat)
    ├── Небоевое здание
    │   ├── Производящее: BARRACKS, TRAINHAL, LIGHTPLT, LANDPLT, AIRPLT, SHIPYARD, SUPRTPLT
    │   ├── Непроизводящее: COMMTWR, POWERSTN, POWGEN, SHIELDGN, RADAR, ADUMP, FDUMP, GOLDSM,
    │   │                   DEPOT, HANGAR, DOCK, HABITAT, RESEARCH, GREENHSE, RECCENTR, MININGST,
    │   │                   LANDPAD, WTRPLTFM, ROAD, BRIDGE, BLOCK
    │   └── Коннектор (инфраструктура комплекса): CNCT_4W
    └── Небоевой мобильный
        ├── Строитель (наземный): CONSTRCT, ENGINEER, BULLDOZR
        ├── Разведчик (наземный): SCOUT, SURVEYOR, SCANNER
        ├── Снабжение (наземный): SPLYTRCK, FUELTRCK, GOLDTRCK, REPAIR
        ├── Миноукладчик: MINELAYR (наземный), SEAMNLYR (морской)
        ├── Транспорт: AIRTRANS (воздушный), CLNTRANS (наземный), SEATRANS, CARGOSHP (морской)
```

### Оси классификации юнитов

| Ось | Значения | Rust-имя (маркер/признак) | C++ |
|---|---|---|---|
| Боевая роль (верхний уровень) | Боевой / Небоевой | `Combat` / `NonCombat` | — |
| Форма | Здание / Мобильный / Стационарный-прочий | `Building` / `Mobile` / `Stationary` | `STATIONARY` / `MOBILE_*` |
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
- Конкретное соответствие «тип → класс/ось» ведётся в `domain-map.md`.
