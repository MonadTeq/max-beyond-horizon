# Ручной прогон 2026-11-26: семь дефектов (взрывы, путь юнита, меню, курсоры, ракеты, эффекты) — **сделано**

Источник: ручной прогон владельца (7 дефектов). Сверка — `max-cpp-port` (Оракул Klei) и
`max-original`/`max-original-dasm` (Оракул Interplay); оракулы не изменялись.

Формат каждого пункта: **Оракул** → **Расхождение** → **Исправление** → **Тест**.

---

## 1. Анимация взрывов зданий/юнитов слишком быстрая

**Оракул.** `UnitsManager_ProgressExplosion` (`max-cpp-port/src/units_manager.cpp:4006-4036`):
кадр листается только при `moved & 0x01`, то есть **раз в два тика обработки приказов**;
тик задан `GameManager_IsTickDue` (`game_manager.cpp:5103`):
`TickTimer_HaveTimeToThink(TIMER_FPS_TO_MS(24))` и `TIMER_FPS_TO_MS(fps)=1000/fps`
(`timer.h:27`) → 42 мс. Значит кадр живёт `2 × 42 ≈ 83 мс`: `LNDEXPLD` (14 кадров,
`frames.rs`) — `28 × 42 ≈ 1.17 с`, `BLDEXPLD` (28 кадров) — ~2.3 с.

**Расхождение.** Шаг симуляции порта — фиксированные 60 Гц
(`max_app/presentation.rs:26-29`), а `progress_explosion` листал кадр по `moved & 1`,
то есть раз в два **своих** шага: 28 × 16.7 ≈ 0.47 с — в 2.5 раза быстрее оригинала.

**Исправление.** `unit_order::EXPLOSION_FRAME_STEPS = 5` (один кадр на пять шагов 60 Гц
= те же ~83 мс); `progress_explosion` сравнивает `moved % EXPLOSION_FRAME_STEPS`.
Тот же счётчик используют смертельная анимация `INFANTRY`/`COMMANDO`
(`units_manager.cpp:3932-3946`) и частицы `RKTSMOKE`/`TRPBUBLE` — они замедлились так же.

**CRC.** `moved`/`image_index` и время жизни эффекта входят в снапшот: сценарии с
боем меняют `state_checksum`/digest'ы (`factory`, `combat` в `phase5_ai_determinism.rs`
перепинены, запись в `ai-port-status.md`). Golden `combat-tb`/`missile-line` переснят.

**Тест.** `max_core/tests/phase2_frames.rs::explosion_frames_advance_every_five_ticks_and_despawn`
(13 кадров `LNDEXPD` = 65 шагов, `removed_at == 65`), `phase2_unit_order`, `turn_based_combat`,
`unit_order_tests::explode_of_dying_infantry_reaches_destroy`, `game_tests::scripted_combat_replay_is_deterministic`.

## 2. Путь юнита: стрелки-подсказки ходов и автодвижение

**Оракул.** `GameManager_RenderScanRangeIndicators` (`game_manager.cpp:936-951`):
`unit->path->Draw(unit, window)` для выбранного своего видимого юнита;
`GroundPath::Draw` (`groundpath.cpp:436-556`) считает остаток очков движения
(`limited_speed = speed*4 + move_fraction`, диагональ — ×1.5) и рисует
`Paths_DrawMarker` (`paths.cpp:376-395`) **зелёным** (`COLOR_GREEN`), пока очки этого
хода не кончились, и **синим** (`COLOR_BLUE`) для клеток следующих ходов; маркер —
треугольник из трёх линий по таблицам `Paths_8DirPointsArrayMarkerA/B/C`
(`paths.cpp:37-44`). `AirPath::Draw` (`airpath.cpp:297-360`) — то же для воздушных.
Приказ, отданный в свой ход, исполняется в конце хода: `GameManager_ProcessTick`
(`game_manager.cpp:5144`) вызывает `UnitsManager_ProcessOrders` только когда
`GameState != GAME_STATE_11_TURN_ACTIVE`.

**Расхождение.** В порте не было стрелок — только зелёная линия после клика
(`hud::draw_path_preview`), без деления на «этот ход / следующие» и без отрисовки
уже поставленного пути.

**Исправление.** `unit_order::path_markers` (порт `GroundPath::Draw`/`AirPath::Draw`,
таблицы `PATH_MARKER_POINTS`), фасад `StateRef::path_markers`;
`render::draw_path_markers` рисует треугольники цветами палитры окон
(`COLOR_GREEN` = слот 2, `COLOR_BLUE` = слот 3) с масштабом зума как в C++
(`offset * zoom / 64`); `render::draws_unit_path` повторяет условия оракула.
Отрисовка включена и в окне 1:1 (`render_map`), и в процедурном HUD
(`hud::render_hud`, цвета `PREVIEW_COLOR`/`PREVIEW_COLOR_BLUE`). Линия после клика
осталась только как переходный вариант, пока тик не применил приказ.

**Наблюдение (не входило в объём).** В порте `GameStateType::TurnActive` не
выставляется нигде (`game.rs`), поэтому шаг симуляции обрабатывает приказы и во время
планирования: юнит начинает идти сразу после клика, а не в конце хода. Это отдельное
расхождение модели хода со всем циклом приказов — требует отдельной задачи
(влияет на все сценарии, CRC и golden); здесь пути и исполнение приказа при закрытии
хода покрыты тестом, но gating `TurnActive` не менялся.

**Тест.** `max_app/tests/playtest_fixes.rs::route_arrows_split_this_turn_from_later_turns`
(12 клеток, часть зелёная, часть синяя, цель всегда зелёная, все стрелки на восток,
в кадре есть цвета слотов 2 и 3);
`an_ordered_route_survives_the_turn_end_and_is_walked` (после клика появляются стрелки,
End Turn не теряет приказ, юнит проходит маршрут).

## 3. Скролл карты не закрывал меню команд

**Оракул.** `GameManager_UpdateMainMapView` (`game_manager.cpp:1745-1775`): после
**успешного** `MAP_VIEW_ZOOM`/`MAP_VIEW_CENTER`/`MAP_VIEW_SCROLL` вызывается
`GameManager_DeinitPopupButtons(false)` (`:1769`).

**Расхождение.** Порт закрывал меню команд только по Esc/пункту меню; карта
прокручивалась под висящими кнопками.

**Исправление.** `App::deinit_popup_buttons()`; вызовы — в `update_scroll`
(только когда камера реально сдвинулась), в `follow_selected_unit`, в кликах по
миникарте (оба режима HUD) и в кнопках зума/центра панели (`ui.rs`).

**Тест.** `playtest_fixes.rs::scrolling_the_map_closes_the_unit_menu`,
`a_hover_without_scrolling_keeps_the_menu_open`, `zooming_closes_the_unit_menu`.

## 4. Курсор-рука над кликабельными элементами меню

**Оракул.** `Cursor_GetDefaultWindowCursor` (`cursor.cpp:57-73`) — все окна
(меню, панели, попапы) дают `CURSOR_HAND` (`HANDPTR`), свои курсоры есть только у
карты (`CURSOR_MAP`) и полос прокрутки; `popupmenu.cpp:40` — `Cursor_SetCursor(CURSOR_HAND)`.

**Расхождение.** Попап команд юнита/здания висит **над окном карты**, и
`session_cursor` отдавал для него курсор карты (`UNIT_GO`/`MAP`), а не руку.

**Исправление.** `cursor::open_window_cursor`: попап команд (`UnitMenu::rect`) —
рука над доступным пунктом; модальные окна (`BuildMenu`, `TransferMenu`,
`AllocMenu`, `UpgradeMenu`, `ResearchMenu`, `UnitStats`, самоуничтожение, пауза,
save-load, preferences, help) — рука по всему окну (как `Cursor_SetCursor(CURSOR_HAND)`
в конструкторах меню).

**Тест.** `playtest_fixes.rs::the_cursor_is_a_hand_over_enabled_menu_items_and_no_go_over_disabled`,
`the_cursor_is_a_hand_over_panel_buttons_and_no_go_over_inactive_areas`.

## 5. Запрещающий курсор над неинтерактивными элементами

**Оракул.** Запрещающий вид — `CURSOR_UNIT_NO_GO` (`UNIT_NGO`, красный знак);
C++ отдаёт его для недоступных действий (`GameManager_GetWindowCursor`,
`game_manager.cpp:4242-4310`: `CURSOR_UNIT_NO_GO` при недопустимом приказе).

**Расхождение.** Над неактивными пунктами попапа и над неинтерактивными зонами окна
(статистика, журнал, координаты, счётчик хода) порт показывал руку/курсор карты.

**Исправление.** Недоступный пункт попапа (`UnitMenuItem::enabled == false`) →
`CursorId::UnitNoGo`; точка вне карты и вне кнопок/миникарты в окне 1:1 →
`CursorId::UnitNoGo` (интерактивные элементы — только кнопки панели, миникарта,
кнопки HUD и попапы).

**Тест.** те же два теста курсоров (ветка `disabled` и зона `STAT_WINDOW`).

## 6. Ракеты летят задом-наперёд

**Оракул.** `UnitInfo::PrepareFire` (`unitinfo.cpp:4749-4788`):
`unit_angle = UnitsManager_GetFiringAngle(...)` — **16 направлений** (`units_manager.cpp:1767`,
0..15), для `ALNMISSL` дополнительно `× 2` (`:4783-4785`, `D_ALNMISSL` — 32 кадра);
`UnitsManager_DeployUnit(..., unit_angle, ...)` → `UnitInfo::UnitInfo` хранит угол как
есть (`unitinfo.cpp:188`) и рисует `image_index = image_base + angle` (`:285`).
`UnitInfo::Attack` делит угол на два для цели (`:2730`). Снаряд не поворачивает:
`AirPath` берёт `m_angle = unit->angle` (`airpath.cpp:52-59`), `Paths_UpdateAngle`
возвращает `false` при совпадении.

**Расхождение.** Порт сохранял `particle.angle = angle & 0x07`: угол выстрела
схлопывался в 8 направлений, выстрел на юг (8) рисовался кадром «север» (0) —
снаряд летел задом-наперёд; `Attack` получал `angle / 2` от усечённого угла.
Дополнительно `render::unit_frame_index_for_angle` маскировал угол `% 8`, а
`App::visual_angle` подменял угол снаряда 8-направленным углом визуального сегмента.

**Исправление.** `combat::prepare_fire` сохраняет 16-направленный угол как есть;
`unit_frame_index_for_angle` использует угол без маски (кадр `image_base + angle`,
как C++, с ограничением `min(last)`); `util::visual_angle` для
`MISSILE_UNIT` всегда отдаёт угол запуска (снаряд не поворачивает).

**Тест.** `max_core/src/combat_tests.rs::a_projectile_keeps_the_sixteen_direction_firing_angle`
(4 стороны + диагональ + `ALNMISSL` = 16), `max_core/tests/turn_based_combat.rs`
(следы/детерминизм), QA `missile-line` (`assert missile-line`: отклонение ≤ 1.5 px).

## 7. Эффекты считаются юнитами в радаре

**Оракул.** `UnitInfo::SpotByTeam` (`unitinfo.cpp:2833-2834`) вызывает `RadarPing()`,
а `UnitInfo::RadarPing` (`unitinfo.cpp:3711-3717`) фильтрует уведомление:
`orders != ORDER_BUILD && orders != ORDER_CLEAR &&
(!(flags & STATIONARY) || unit_type == LANDMINE || unit_type == SEAMINE) && (flags & SELECTABLE)`
→ `GameManager_NotifyEvent(this, 0)` = «Enemy %s in radar range.»
(`game_manager.cpp:3446-3469`). Эффекты (`LNDEXPLD`…`HITEXPLD`, `RKTSMOKE`,
`TRPBUBLE`, обломки) имеют в `SC_UNITS` `STATIONARY` и не имеют `SELECTABLE`
(`access.rs::UNIT_TYPE_PROPERTIES`, `0x0000_0206`).

**Расхождение.** Порт отправлял отчет `EnemySpotted` для **каждого** `SpotByTeam`
команды игрока: каждая очередь/взрыв давали «юнит "взрыв"/"пузырьки" в зоне радара»
(видно в прежнем golden `combat-tb-trail.png`: «Enemy HITEXPLD in radar range.»).

**Исправление.** `ai::radar_ping_reports` — точный порт предиката `RadarPing`;
`GameState::spot_by_team` пушит `spotted_reports` только при нём.

**Тест.** `max_core/tests/enemy_halt.rs::spotted_effects_do_not_ping_the_radar`
(7 типов эффектов) и `a_spotted_enemy_tank_still_pings_the_radar` (боевой юнит — пингует).

---

## Валидация

* `cargo check --workspace` — зелёный.
* `cargo nextest run -p max_core -p max_app --no-default-features` — **1868 тестов, 0 падений**
  (сетевой `p2p::gate_g_*` — известный flaky, прошёл на повторе).
* QA: все 23 сценария `max-brh-qa/scripts/*.qa` — **PASS**; `combat-tb`/`move-input` — PASS,
  golden `combat-tb`, `missile-line`, `move-input`, `unit-menu`, `panels` — 0 пикселей дрейфа.
* Перепин golden с обоснованием: `combat-tb` (11 кадров) и `missile-line` (3 кадра) —
  изменились кадры взрыва (дефект 1), ориентация снаряда (дефект 6) и исчезло ложное
  уведомление радара (дефект 7); на `combat-tb-trail` видно исчезновение
  «Enemy HITEXPLD in radar range.».
* `check_render_readonly.py` — PASS (слой рендера read-only).
* **Замечание вне объёма:** golden `sp-tb-turn1` даёт дрейф 1.03 % **независимо** от этих
  правок (проверено поочерёдным откатом `EXPLOSION_FRAME_STEPS`, фильтра `RadarPing`,
  маски угла в рендере и стрелок пути — дрейф не меняется); эталон не переснимался,
  чтобы не маскировать отдельную проблему кадра.
