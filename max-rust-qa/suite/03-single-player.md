# 03 — Одиночная игра (TC-03xx)

Локальная партия 1 человек против 1–3 ИИ.

| ID | Название | Шаги | Ожидание | Автоматизация |
|---|---|---|---|---|
| TC-0301 | Старт партии | Players: RED=Human, GREEN=Computer → Done | партия стартует: карта выбранной планеты (WRL-тайлы), стартовые юниты (шахта/станция/конструктор/генератор…) развёрнуты | ✅ QA sp-full.qa; UNIT local_sp |
| TC-0302 | Развёртывание 1:1 с C++ | сравнить размещение стартовых юнитов | RNG-порядок и позиции как в C++ (deploy-тесты) | ✅ UNIT deploy тесты |
| TC-0303 | Ход игрока | первый ход RED | таймер тикает, управление активное, ИИ ждёт | ✅ QA |
| TC-0304 | Ход ИИ | End Turn → ход GREEN (ИИ) | ИИ ходит сам (задачи: строительство/разведка), ввод игрока заблокирован | ✅ QA sp-full (3 хода); UNIT local_sp |
| TC-0305 | Порядок раунда | партия с 2 ИИ | RED→GREEN→BLUE, None/Eliminated пропускаются | ✅ UNIT |
| TC-0306 | Таймер хода | таймер из настроек; истечение | ход закрывается автоматически по таймеру | ✅ UNIT |
| TC-0307 | Пауза | P / кнопка паузы | тики и таймер стоят; повторное нажатие — продолжение | ✅ UNIT |
| TC-0308 | Экономика/производство | несколько ходов | золото копится, производство списывает ставку `Cargo_GetRawConsumptionRate` за ход (по C++-формулам) | ✅ UNIT production (ставки/цены); QA production.qa |
| TC-0309 | Бой с ИИ | подвести юнита к врагу, атаковать | урон по формулам C++, реактивный огонь, уничтожение | ⏭ not planned: механика покрыта UNIT combat (урон/реактивный огонь/уничтожение); сквозной бой в QA требует команды урона/телепорта (вне P6) |
| TC-0310 | ИИ строит юниты/здания | наблюдать ИИ несколько ходов | ИИ создаёт здания/юниты через `TaskManager` → очередь `ProductionManager` → `SpawnNewUnit` (не мгновенный спавн) | ✅ UNIT ai_p6 (`ai_completes_a_build_queue_through_production`) |
| TC-0311 | Производство игрока | конструктор → `B` → BuildMenu → заказ TRAINHAL → 10 ходов | очередь производства, списание золота по ставке C++, спавн здания в конце хода; затем завод строит INFANTRY за 2 хода | ✅ QA production.qa; UNIT production; golden `golden/production/buildmenu.png` |
| TC-0312 | Исследования | апгрейды/исследования | работают по C++ | ✅ QA `research.qa`: конструктор строит Research Center (12 ходов), `O` включает питание (`Power On` → пакет 08/P1), тема Attack за 16 ходов даёт уровень 1 (`ResearchMenu_NewTurn`); UNIT `max_core/tests/research.rs` — формулы стоимости/факторов/дельт бит-в-бит против C++-векторов (`tools/upgrade_vectors`); экран распределения тем (RSRCHPIC) не портирован — allocation задаётся питанием центра/API |
| TC-0313 | Победа по очкам (Duration) | победа через Duration=1 | экран победы 1:1 (ENDGAME), «1st place» у человека | ✅ QA victory.qa |
| TC-0314 | Поражение | человек элиминирован | экран поражения | ⏭ not planned: UNIT winloss (экран/места) покрыт; сквозной сценарий поражения требует команды урона (как TC-0309) |
| TC-0323 | Покупные апгрейды | выделить юнита → `U` → Upgrade-экран → `Done` | ✅ QA `upgrade.qa` + golden `golden/upgrade/upgrade-menu.png`; золото списывается только по `Done`, `Cancel` откатывает; UNIT `max_app/tests/upgrade.rs` (current_values и новые юниты получают апгрейд) |
| TC-0315 | Сохранение во время партии | сохранить/загрузить | состояние восстанавливается (MXLS контейнер; полный V70 — см. TC-07xx) | ✅ UNIT |
| TC-0316 | Мобилизация (CargoMenu) | Players Done → экран мобилизации; купить юнитов, Done | CARGOPIC, список 15 юнитов, цены `3 × ATTRIB_TURNS`, кредиты уменьшаются, загрузка растёт; при `start_gold = 0` фазы нет | ✅ QA sp-full/hs-2; UNIT mobilize; golden mobilize.png |
| TC-0317 | Выбор точки высадки (SelectSite) | мобилизация Done → карта 4 px/клетка, клик по клетке, Done | точка применяется: база высаживается у выбранной клетки, 4×4-суши; невалидная клетка отклоняется | ✅ QA sp-full/hs-2; UNIT start_phases; golden landing.png |
| TC-0318 | ИИ сам выбирает точку | партия с ИИ после фаз людей | ИИ получает точку `Ai_SelectStartingPosition` (5×5 суши, ≥ `AI_SAFE_ENEMY_DISTANCE`), не четверть карты | ✅ UNIT start_phases; UNIT max_core setup |
| TC-0319 | Порядок фаз hot-seat | hot-seat 2 человека → Done | фазы идут GRAY→RED (у нас GREEN→RED): мобилизация+высадка на каждого человека | ✅ QA hs-2; UNIT start_phases |
| TC-0320 | Кредиты после мобилизации | купить юнитов и закрыть Done | `TeamUnits::SetGold` = остаток; состав = `SC_L0001` + покупки | ✅ UNIT start_phases |
| TC-0321 | BuildMenu завода (очередь) | завод → `B` → список → `Build` (в очередь) → `Done` | очередь `FACBUILD` (337,60 116×220), `Delete`/`Repeat`, заказ уходит `PayloadP4` и повторяется при `Repeat` | ✅ QA production.qa; UNIT buildmenu |
| TC-0322 | Отмена постройки | заказ → `Cancel`/исчерпание золота | `CancelBuilding`: очередь очищается, `build_time = 0`; при нехватке золота постройка встаёт на паузу, заказ не теряется | ✅ UNIT production (`cancel_stops_the_construction`, `missing_gold_pauses…`) |
| TC-0324 | Мобилизация: изображения и индикаторы | экран мобилизации, любой список | иконки `I_*` 32×32 в строках (мобильные 1:2, здания 1:4), у турельных типов — второй кадр башни со сдвигом `angle_offsets[2] >> divisor`, портрет `P_*` 300×240 в (11,13) с обрезкой окном 280×240, описание `GetDescription()` (16,17 280×230, шрифт 5 + обводка) и таблица `UnitStats_DrawStats` (строки 19 px, pips `I_*`, красные линии), полоса золота `VERTGOLD` (361,276 40×140), cargo-слайдер `VERTRAW/FUEL/GOLD` (421,301 20×115), счётчик cargo (421,276) | ✅ QA `cargo-full.qa` (`cargo-description/cargo-portrait/cargo-stat-icons`) + golden `golden/cargo/cargo-buy.png`, `cargo-description.png`; UNIT `cargo_render`, `mobilize`, `cargo_unit_data` |
| TC-0325 | Мобилизация: полный список покупки | список Available | 15 типов `CargoMenu::unit_types1`, 9 видимых строк, прокрутка `BLDUP__/BLDDWN`, цены `3·turns` | ✅ C++-векторы `tools/cargo_vectors` (бит-в-бит); QA `cargo-full.qa` (`cargo-types=15`) |
| TC-0326 | Мобилизация: режим модернизации | переключатель `CRGTOG` (542,446) | полный список `IsUnitFiltered` (62 типа по умолчанию), фильтры `SELGRD/SELAIR/SELSEA/SELBLD/SELCBT`, строки `UPGLFT/UPGRGT` с ценами `UpgradeControl_CalculateCost` | ✅ C++-векторы (списки/строки/цены); QA `cargo-full.qa` + golden `cargo-upgrade.png` |
| TC-0327 | Мобилизация: апгрейд до высадки | ряд апгрейда → `Done` | кредиты списаны, `UnitValues` типа изменены, высаженные юниты получают новые значения | ✅ UNIT `start_phases::mobilization_upgrades_reach_the_deployed_units`; QA `cargo-full.qa` (`gold<150`) |
| TC-0328 | Мобилизация: cargo в юниты | выбрать строку «Purchased», слайдер cargo | 1 кредит за 5 единиц, ёмкость `storage/5` (GOLDTRCK — 0), бесплатный cargo миссии не снимается, продажа возвращает `cargo/5` | ✅ UNIT `mobilize::tests`; QA `cargo-full.qa` |

Итог: 28 TC, ✅ 26, 🟡 0, ⏭ 2 (TC-0309 сквозной бой, TC-0314 сквозное поражение).
