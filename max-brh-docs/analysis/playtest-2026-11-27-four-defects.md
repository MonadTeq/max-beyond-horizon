# Ручной прогон 2026-11-27: четыре дефекта (перехват, зум колесом, бегунок, клетка 001-001)

Источник: ручной прогон владельца (4 дефекта). Сверка — `max-cpp-port` (Оракул Klei),
`max-original`/`max-original-dasm` (Оракул Interplay); оракулы не изменялись
(автосейв QA-прогонов `max-original/save10.*` восстановлен к состоянию коммита).

Формат каждого пункта: **Оракул** → **Расхождение** → **Исправление** → **Тест** → **Итог**.

---

## 1. Перехват движущегося юнита: враг стреляет, юнит ОСТАНАВЛИВАЕТСЯ

**Оракул.**

* Закрытие хода: `GameManager_TeamTurnTurnBased`/`GameManager_TeamTurnConcurrent`
  (`max-cpp-port/src/game_manager.cpp:488-490`, `:549-558`) —
  `while (Access_AreTaskEventsPending()) GameManager_ProgressTurn();`;
  `GameManager_ProgressTurn` (`:2586`) — это полный `GameManager_ProcessTick(false)`,
  а он вызывает `UnitsManager_ProcessOrders()` (`:5182`), т.е. каждый проход —
  движение + `UnitsManager_AssessAttacks` (реакционный огонь) + обработка
  `UnitEvent_UnitEvents` (экстренные остановки).
* `Access_AreTaskEventsPending` (`access.cpp:2269`): в том числе список
  `UnitsManager_DelayedAttackTargets` (реакционный обмен) и `ORDER_STATE_INIT`.
* Экстренная остановка: `Access_UpdateMapStatus` (`access.cpp:827-868`) ставит
  `UnitEventEmergencyStop` движущемуся юниту, который обнаружил опасного врага
  (`enemy_halt`); `UnitEventEmergencyStop::Process` (`unitevents.cpp:38`) →
  `UnitInfo::StopMovement` (`unitinfo.cpp:3222`) → `GroundPath::CancelMovement`
  (`groundpath.cpp:161`) → `UnitInfo::BlockedOnPathRequest()` (по умолчанию
  `mode = true`, `unitinfo.hpp:306`, `unitinfo.cpp:3628`) → `MoveFinished(true)`
  (`unitinfo.cpp:3654`) → `UnitsManager_AddToDelayedReactionList`
  (`unitinfo.cpp:3692-3694`): остановленный юнит становится целью реакции.
* Реакция: `UnitsManager_CheckReaction` (`units_manager.cpp:4709`) —
  `shots > 0`, нет `delayed_reaction`, `Access_IsWithinAttackRange`,
  `Access_IsValidAttackTarget`, `UnitsManager_ShouldAttack` (`:4623`: цель видима
  стрелку); `UnitsManager_CheckDelayedReactions` (`:4888`) обходит команды и
  стреляет по цели из списка (включая дозорных — второй цикл
  `IsTeamReactionPending`).

**Расхождение.** В порте было два дефекта.

1. `GameState::finish_end_turn_movement` вызывал только `unit_order::process_order`
   для юнитов закрывающейся команды и **не обрабатывал `unit_events`** и не запускал
   `combat::assess_attacks`. `UnitEventEmergencyStop`, поставленный при входе в зону
   врага, лежал в очереди до следующего тика: юнит проходил ВЕСЬ маршрут, а на
   конечной точке `MoveFinished(true)` добавлял его в список целей — враг стрелял
   уже на пункте назначения (симптом: «долетает до точки назначения и взрывается
   на конечной точке, игнорируя атаку»).
2. `events::process_unit_event` вызывал только `stop_movement` уровня юнита
   (`BlockedOnPathRequest`/`MoveFinished` без игрового состояния), поэтому даже
   обработанная остановка **не добавляла юнита в `UnitsManager_DelayedAttackTargets`**:
   остановка была, а реакции не было вовсе.
3. Сверх оракульного `enemy_halt` (остановка при *обнаружении* врага) не было
   перехвата в момент, когда уже разведанный враг берёт движущегося юнита в
   **зону поражения** и видит его: `enemy_halt` срабатывает только на *новом*
   раскрытии клетки (`HeatMap::Add` → `Access_OnCellRevealed`), а
   `UnitsManager_CheckReaction` для такого врага уже был бы истинным.

**Исправление.**

* `max_core/src/events.rs:96` — `process_unit_event` повторяет путь оракула:
  `StopMovement`; при остановке из `ORDER_STATE_IN_PROGRESS` —
  `add_to_delayed_reaction_list` (аналог `MoveFinished(true)`), `Ai_CheckMines` и
  `Ai_EvaluateAttackTargets`.
* `max_core/src/game.rs:2059` (`finish_end_turn_movement`) — каждый проход теперь
  полный пасс приказов (`process_orders`): движение, `AssessAttacks`, обработка
  `UnitEvent_UnitEvents`; остановка применяется до следующей клетки. Предикат
  цикла (`task_events_pending`, `:2130`) сужен до маршрутов закрывающейся команды,
  которые сейчас проходятся (`path.is_some()` + `IN_PROGRESS`/`IN_TRANSITION`):
  реакционный обмен разрешается на следующем пассе приказов (через 1 тик), как и
  раньше — это сохраняет тайминги выстрелов/AI-хода (`ai_turn_timer`,
  `combat-tb`), при этом остановленный юнит уже в списке целей и враг стреляет
  именно в клетке остановки. Отличие от оракула зафиксировано в doc-комментарии.
* `max_core/src/game.rs:996` (`check_movement_interception`) — при смене клетки
  движущимся юнитом проверяется реакция врага из его списков
  (`MobileLandSea`/`MobileAir`/`Stationary`): `shots > 0`, нет `delayed_reaction`,
  юнит видим этой команде, в пределах `ATTRIB_RANGE`, валидная цель,
  `ShouldAttack` → ставится `EmergencyStop` (дедупликация). Гейтится
  `enemy_halt` и `disable_fire`; ракеты (`MISSILE_UNIT`) не перехватываются.
* Симуляция меняется (CRC): добавлены остановки/реакции, которых раньше не было.
  Существующие CRC-эталоны и golden не затронуты, потому что ни один golden-сценарий
  не содержит перехвата у границы хода; проверено полным прогоном.

**Тест.** `max_core/tests/intercept_reaction.rs` (3):

* `the_turn_close_stops_a_mover_in_the_enemy_range_and_fires_at_the_halt_cell` —
  танк RED идёт 8 клеток на танк GREEN: `end_local_turn` останавливает его на
  `20 − ATTRIB_RANGE` (не на пункте назначения), затем `AssessAttacks` наносит урон
  именно в клетке остановки;
* `a_scouted_enemy_still_intercepts_the_mover` — враг уже разведан
  (`SpotByTeam`), оракульный `enemy_halt` молчит; перехват всё равно срабатывает;
* `enemy_halt_off_lets_the_mover_reach_the_destination` — контроль: с выключенной
  настройкой маршрут проходится до конца (поведение до фикса).

**Итог.** Юнит останавливается на входе в зону поражения видимого врага, враг
открывает огонь в клетке остановки. Тесты зелёные; QA `combat-tb`/`move-input`
PASS, golden без перепина.

---

## 2. Масштаб карты колёсиком мыши

**Оракул.** `GameManager_ProcessInput` (`max-cpp-port/src/game_manager.cpp:6404-6414`):
при `GameManager_IsMainMenuEnabled` — `mouse_get_wheel_delta(&dx, &dy)`, при
`dy != 0` → `GameManager_UpdateMainMapView(MAP_VIEW_ZOOM, Gfx_ZoomLevel + dy, …)`.
`mouse_get_wheel_delta` (`mouse.c:323-325`) отдаёт накопленные «щелчки» ×
`mouse_wheel_sensitivity` (по умолчанию 3, `settings.cpp:109`);
`mouse_add_wheel_event` (`:312`) квантует float-дельту с остатком
(`mouse_quantize_wheel_axis`, `:302`). Зажим — `GameManager_ZoomMainMapView`
(`game_manager.cpp:1699-1700`): `MainmapZoomLimit…64` (в порте `MIN_ZOOM…64`).
Реальный зум — это `MAP_VIEW_ZOOM`: закрывает попап команды
(`GameManager_DeinitPopupButtons`, `:1769`).

**Расхождение.** Событие колеса вообще не доходило до UI: `WindowInput`/
`InputEvent` не имели колеса, `SdlWindow::poll_input` игнорировал
`SDL_EVENT_MOUSE_WHEEL`, у камеры не было обработчика.

**Исправление.**

* `max_render/src/sdl_window.rs` — `WindowInput::MouseWheel { notches }`,
  квантование `quantize_wheel_axis` (`:176`, аналог `mouse_quantize_wheel_axis`),
  обработка `Event::MouseWheel` (`:1135`);
* `max_app/src/main.rs` — `WindowInput::MouseWheel → InputEvent::MouseWheel`;
* `max_app/src/ui.rs` — вариант `InputEvent::MouseWheel(i32)` (`:185`),
  `session_mouse_wheel` (`:3684`): `zoom + notches * WHEEL_ZOOM_STEP` (3),
  зажим `MIN_ZOOM…64`, `deinit_popup_buttons` только при реальном изменении;
* `max_app/src/panels.rs:1489` — `WHEEL_ZOOM_STEP = 3`.

Колесо обрабатывается до проверки попапа команд (как C++ при
`IsMainMenuEnabled`), но после модальных окон (`DisableMainMenu` их глотает).

**Тест.** `max_app/tests/playtest_fixes.rs::the_mouse_wheel_zooms_the_map` — один
щелчок вниз −3, вверх +3, верхний зажим 64, нижний `MIN_ZOOM` (8), попап
закрывается; `max_render/src/sdl_window_tests.rs::wheel_deltas_quantize_into_notches_with_a_remainder`
— остаток квантования (¼+¼+¼+¼ = 1 щелчок), отрицательное направление, 2 щелчка
за событие.

**Итог.** Колесо меняет масштаб карты 1:1 с оракулом; поведение симуляции не
меняется (CRC не затронут).

---

## 3. Калибровка бегунка масштаба

**Оракул.** `GameManager_UpdateZoomSlider` (`game_manager.cpp:1731-1740`):
`offset = ((lrx − ulx − width) × (64 − zoom)) / 60 + width / 2`, затем
`WindowManager_LoadSimpleImage(ZOOMPTR, offset + window.ulx, window.uly + 1, true)`.
`WindowManager_DecodeSimpleImage` (`window_manager.cpp:784-785`) вычитает hotspot
самой картинки: `ulx -= image->ulx; uly -= image->uly;`. `ZOOMPTR` (`MAX.RES`) —
25×14, hotspot `ulx = 12`; значит центр/хотспот бегунка попадает ровно в
`window.ulx + offset`, а левый край картинки — на 12 px левее.

**Расхождение.** Порт брал формулу правильно (`zoom_slider_offset` =
`101·(64−zoom)/60+14`), но блиттил `ZOOMPTR` в `ZOOM_SLIDER.ulx + offset` **без
вычитания hotspot** (в отличие от `DecodeSimpleImage`). Бегунок стоял на 12 px
правее позиции трека; в крайнем левом положении (зум 64) выглядел «съехавшим
вправо».

**Исправление.** `max_app/src/panels.rs:1516` — `zoom_pointer_position(zoom,
pointer_ulx, pointer_uly)` = `(slider.ulx + offset − pointer_ulx, slider.uly + 1 −
pointer_uly)`; отрисовка (`:1100`-е) использует её с реальными `ulx/uly` ресурса.

**Тест.** `max_app/src/panels_tests.rs::zoom_pointer_hot_spot_lands_on_the_slider_position`:
хотспот `left + 12` равен `slider.ulx + zoom_slider_offset(64)` (и клик по нему
даёт 64), бегунок целиком внутри окна 130 px на зумах 8/32/64.

**Итог.** Бегунок совпадает с позицией трека и с обратным преобразованием
`zoom_from_slider_x`. Презентация; golden-дрейф — только область бегунка
(0.066–0.370 % при пороге 1 %).

---

## 4. Клетка 001-001 видна в окне карты (не под панелью)

**Оракул.** `WindowManager_ScaleWindows` (`window_manager.cpp:202-230`):
`WindowManager_MapWidth/MapHeight` — размер `WINDOW_MAIN_MAP` внутри экрана
(448×448 при (180,18) в окне 640×480). `GameManager_CenterMainMapView`
(`game_manager.cpp:1680-1696`) зажимает центр по `WindowManager_MapWidth/Height`,
а `GameManager_UpdateMapDrawBounds` (`:1568-1626`) — по пиксельным границам
карты: левый/верхний край `ulx = 0`, т.е. первая клетка начинается ровно у края
окна карты.

**Расхождение.** Камера приложения (`Viewport`) хранилась и зажималась как
640×480 — по всему кадру. При зуме 64 полуширина кадра 320 world-px = 5 клеток,
поэтому крайнее левое/верхнее положение центра — 320, а `map_viewport()` (448×448)
при том же центре даёт origin 96: первые 1.5 клетки карты вообще не попадали в
окно карты — клетка 001-001 «уходила под панель».

**Исправление.**

* `max_render/src/viewport.rs:168` — `Viewport::set_view_size(w, h)`: видимая
  область камеры отделена от размера кадра; `origin`/`clamp_center`/`visible_tiles`
  используют её (`:176`, `:260`, `:352`), `scaled` масштабирует вместе с кадром;
* `max_app/src/lib.rs:3640-3650` — после загрузки ассетов при активном 1:1-хроме
  view = 448×448 (`WINDOW_MAIN_MAP`), иначе — кадр из конфигурации. Камера может
  дойти до origin (0,0) и до правого/нижнего края (origin = 7168 − 448).

**Тест.**

* `max_render/src/viewport_tests.rs::a_smaller_map_view_lets_the_camera_reach_the_map_edge`
  — центр 224, origin (0,0), первая клетка в (0,0), последняя заканчивается на 448;
* `max_app/tests/map_camera.rs` — с настоящими ассетами (`init_assets`,
  `chrome_active`): прокрутка в край → `map_viewport().origin() == (0,0)`, клетка
  `tile_rect(0,0)` целиком внутри `MAIN_MAP`; в противоположном краю последняя
  клетка заканчивается ровно на границе окна карты.

**Итог.** Клетка 001-001 видна в левом верхнем углу окна карты; камера ходит до
всех четырёх краёв. Презентация; golden не меняется (сценарии не у края карты).

---

## Валидация

* `cargo check --workspace` — зелёный.
* `cargo nextest run -p max_core -p max_app --no-default-features` — **1991 тест,
  0 падений** (max_core 953, max_app 1038).
* QA (порог golden Δ > 30 / 1 %):

| сценарий | `qa_run` | golden |
|---|---|---|
| `move-input` | PASS (5429 кадров, 7 PNG) | PASS, 0.066–0.370 % (бегунок) |
| `combat-tb` | PASS (448 кадров, 11 PNG) | PASS, 0.186–0.207 % (бегунок) |
| `panels` | PASS | PASS, 0.447–0.906 % (0.478 % — уже существовавшее расхождение эталона + бегунок) |
| `sp-tb` | PASS | PASS, 0.131 % (бегунок) |
| `ai-turn` | PASS | PASS, 0.725–0.895 % (0.709 % — существовавшее + бегунок; геймплей не менялся: 0.186 % между прогонами) |

* Перепин golden не выполнялся: все сценарии ниже порога 1 %. Дрейф —
  осознанный сдвиг бегунка (12 px) плюс уже существовавшая stale-разница
  `panels`/`ai-turn` (проверена сравнением с прежним `max-brh-qa/out/*`).
* CRC: пункт 1 — законное изменение поведения (остановки/реакции, которых раньше
  не было); конкретные сценарии golden/CRC-эталонов перехвата у границы хода не
  содержат, поэтому эталоны не перепинены. Пункт 2 — ввод, симуляцию не меняет;
  пункты 3–4 — презентация.
* Оракулы не изменялись; автосейв QA-прогонов в `max-original/save10.*` возвращён
  к состоянию коммита.
* **Без коммита** (по заданию).
