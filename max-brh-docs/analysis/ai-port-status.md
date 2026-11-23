# Статус порта ИИ и план фазы A

Результат аудита 1.2.1 (сверка `max_core::ai` с C++-оракулом). Этот документ —
рабочая база для фазы **A** (полный порт ИИ 1:1) и перечень уже закрытых
багфиксов (фаза **B**).

## Итог аудита

Каркас ИИ портирован аккуратно, но **поведенческая часть — в основном
упрощения**, а не 1:1 с C++. Текущий ИИ — упрощённая эвристика.

## Что совпадает 1:1

- перечисление 10 стратегий (`AiStrategy`), маппинг клан→стратегия,
  `RollGreenhouseRatio`/`RollMinefieldDensity`;
- константы приоритетов задач, реестр типов задач (49 имён / 44 класса);
- RNG-алгоритм, диспетчеризация жизненного цикла задач.

## Критичные расхождения (влияют на поведение)

| # | Область | Статус | Что расходится |
|---|---|---|---|
| 1 | Вес юнитов / состав армии | отсутствует | 18 `WeightTable` пустые; `UpdateWeightTables`/`RebuildWeightTables` не вызываются; нет `ChooseStrategic/GenericBuildOrders`, `DetermineResearchProjects`, `ChooseUpgrade` |
| 2 | Выбор стратегии/высадки | расходится | упрощённые критерии континента (`viable=size>=40`, `isolated=size<80`); `target_location` = центр карты; нет `IsViableSite`/`TestIsolated`/`SelectLandingSite`/`Randomizer_Generate(total_continent_size)` |
| 3 | Производство | частично | жёсткая эвристика (шахта/склад/генератор/радар); не строит заводы/армию; нет `PlanNextBuildJob`/`RollTeamMissionSupplies` |
| 4 | Бой | частично | точечная атака в радиусе; нет `EvaluateAssault`/наступления, `GetRequiredDamage`/`GetProjectedDamage` |
| 5 | Разведка | отсутствует | нет целенаправленного исследования карты |
| 6 | Флот | отсутствует | не строит/не использует морские юниты; нет `AttemptTransport` |
| 7 | Исследования/апгрейды | отсутствует | нет выбора исследований и апгрейдов юнитов |
| 8 | Детерминизм | не бит-в-бит | другой порядок решений → при том же seed другие ходы |

## Фаза B — закрытые багфиксы (коммит `b9eb781`)

1. `EndTurn` у Attack/KillUnit вызывал `BeginTurn` → исправлено (`kill_unit_end_turn`/`attack_end_turn`).
2. `create_building_activate` терял `building_id` → исправлено (используется, порт `Activate`/`Finish`/`AddUnit`).
3. `attack_priority` жёстко `ATTACK_DEFAULT` → заменено на `priority_of`; формула `TaskAttack::GetPriority` внедрена.
4. `TaskMineAssistant` получал `CHECK_ASSAULTS` → `FRONTIER` (`taskmineassistant.cpp:27`).
5. Морские мины убирал наземный `MineLayer` → `SeaMineLayer` по `Access_GetModifiedSurfaceType` (`taskremovemines.cpp:101-112`).
6. `TaskAttack::GetPriority`/`TaskGetMaterials::GetPriority` отсутствовали → добавлены (`taskattack.cpp:101-112`, `taskgetmaterials.cpp:39-50`).
7. `TaskUpdateTerrain` получал `CHECK_ASSAULTS` → `EMERGENCY` (`taskupdateterrain.cpp:35`).

CRC/replay без изменений; 385 тестов зелёные.

## Фаза A — полный порт ИИ (по убыванию влияния на геймплей)

1. **Вес-таблицы + состав армии** (`WeightTable` ×18, `UpdateWeightTables`,
   `RebuildWeightTables`) — база всех решений ИИ.
2. **Выбор стратегии/высадки** (`SelectStrategy`, континенты, `SelectLandingSite`,
   детерминированный RNG).
3. **Производство** (`ChooseStrategic/GenericBuildOrders`, `PlanNextBuildJob`,
   заводы/армия, `RollTeamMissionSupplies`).
4. **Бой** (`EvaluateAssault`, `GetRequiredDamage`/`GetProjectedDamage`,
   census целей, `IsReconUnitAvailable`, `IsAttackUnderControl`).
5. **Исследования/апгрейды** (`DetermineResearchProjects`, `ChooseUpgrade`).
6. **Разведка и флот** (`TaskExplore`, `AttemptTransport`, морские задачи).

Каждый пункт — отдельная задача Разработчику (v4 Flash) со сверкой по
`aiplayer.cpp`/`task*.cpp` и фиксацией CRC/replay.

## Открытые вопросы владельца

- Требование к детерминизму бит-в-бит ИИ (п. 8) — цель или допустимо «та же
  стратегия, но другой seed-путь» до полного порта?
- Порядок фаз A: рекомендуемый — по списку выше (вес-таблицы → стратегия →
  производство → бой → исследования → разведка/флот).
