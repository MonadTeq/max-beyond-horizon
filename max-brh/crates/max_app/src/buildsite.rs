//! Режим выбора места стройки — порт `ORDER_STATE_SELECT_SITE` конструктора
//! (`buildmenu.cpp:921`, `game_manager.cpp:4566/5829`).
//!
//! # Что делает C++ (оракул)
//!
//! 1. `MobileBuildMenu::Build` (`buildmenu.cpp:872`): игрок выбрал здание
//!    (`Unit::GetFlags() & BUILDING`) и нажал `Done` → `Build`
//!    (`UnitsManager_IssueBuildOrder`) проверяет четыре якоря 2×2 вокруг
//!    строителя, затем `UnitsManager_SetNewOrderInt(unit, ORDER_BUILD,
//!    ORDER_STATE_SELECT_SITE)` и **спавнит временную «ленту»**
//!    `GameManager_TempTape = UnitsManager_SpawnUnit(LRGTAPE, …, grid_x,
//!    grid_y, unit)` — это и есть призрак площадки (не призрак здания и не
//!    рамка). Сообщение — `_(06cb)`.
//! 2. `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5829`) на каждом
//!    движении мыши: для конструктора 2×2-лента привязывается к строителю
//!    (`site_anchor_for_cursor`) и двигается `UnitsManager_MoveUnit`;
//!    `CURSOR_MAP2` — если `UnitsManager_MoveUnitAndParent` вернул
//!    проходимость **и** курсор попал внутрь 2×2, иначе `CURSOR_UNIT_NO_GO`.
//! 3. `GameManager_PathBuild` (`game_manager.cpp:4566`) по LMB:
//!    `move_to = TempTape->grid`, лента уничтожается, `UnitsManager_StartBuild`
//!    → `ORDER_BUILD`/`INIT`; на следующем тике `UnitInfo::StartBuilding`
//!    ставит настоящую площадку (`LRGTAPE`+`LRGCONES`+`LRGSLAB`) в `move_to`.
//! 4. Отмена — `GameManager_ManagePlayerAction` → `GameManager_SelectBuildSite`:
//!    лента уничтожается, `BuildList` очищается, приказы восстанавливаются.
//!    Так работают RMB и (в C++) клик по невалидной клетке.
//! 5. После `StartBuilding` конструктор проходит `ORDER_AWAIT_TAPE_POSITIONING`
//!    (`UnitInfo::PositionInTape`, `unitinfo.cpp:4414`). Сначала
//!    `Paths_UpdateAngle` доворачивает его к `GetTargetUnitAngle` (для 2×2 —
//!    диагональ к центру, `unitinfo.cpp:4147`); пока поворот не закончен,
//!    `moved` не растёт. Затем четыре тика `OffsetDrawZones` по 8 px двигают
//!    **спрайт** строителя по `DIRECTION_OFFSETS[angle]` на 32 px (половину
//!    клетки) — в центр ленты, лицом по ходу. На завершении C++ доворачивает
//!    юнита на шаг `(angle + 1) & 7` под рабочую позу. Сетка (`grid_x/grid_y`)
//!    при этом не меняется.
//!
//! # Что портировано
//!
//! * Привязка якоря 2×2 к строителю и валидация клетки — `max_core::production`
//!   ([`max_core::production::initial_build_site`],
//!   [`max_core::production::site_anchor_for_cursor`],
//!   [`max_core::production::site_covers`],
//!   [`max_core::production::is_site_accessible`]).
//! * Призрак — **та же декаль** `LRGTAPE`/`SMLTAPE`, что спавнит C++ (рисуется
//!   общим спрайтовым циклом), курсор `CURSOR_MAP2`/`CURSOR_UNIT_NO_GO`
//!   (SDL-слой, как `Cursor_SetCursor`).
//! * LMB — заказ 08/P4 с `move_to` = якорь призрака; площадку
//!   (`LRGTAPE`+`LRGCONES`+`LRGSLAB`) ставит `events::apply_command` в момент
//!   применения пакета — плита видна уже на кадре подтверждения. C++ ставит её
//!   на следующем тике (`StartBuilding`/`INIT`), что в 60 fps незаметно; в
//!   порте `unit_order::start_building` идемпотентен и на следующем тике
//!   только переводит строителя в `PositionInTape`.
//! * RMB/Esc — отмена без заказа (приказ не уходит).
//! * Сдвиг в центр ленты — [`tape_center_offset`]: доворот к ленте
//!   (`unit_order::position_in_tape`), затем 8 px × 4 тика = 32 px по
//!   `DIRECTION_OFFSETS[unit.angle]` (`OffsetDrawZones`), применяется рендером
//!   к якорю спрайта; для `DEINIT`/`LEAVE` — те же 32 px плюс 8 px за тик в
//!   сторону `move_to` (строитель съезжает с площадки перед `ActivateEngineer`).
//!
//! # Осознанные отличия
//!
//! * C++ спавнит временную ленту в мире (`GameManager_TempTape`); порт рисует
//!   её как UI-призрак — состояние партии, счётчик юнитов, CRC и сейв не
//!   меняются, пока игрок выбирает место.
//! * `GetBuilderUnitCursor` для `ENGINEER` (`game_manager.cpp:5854-5909`)
//!   портирован: лента 1×1 стоит в клетке курсора, только в строке/столбце
//!   инженера; включается кнопкой `Path` (`BLDPTH_U`, `buildmenu.cpp:836`).
//!   C++ `UnitsManager_StartBuild` (`units_manager.cpp:1516`) сбрасывает
//!   `move_to` инженера в его клетку — это обнулило бы выбор `Path`, поэтому
//!   порт ставит площадку в выбранную клетку (см. §33).
//! * C++ открывает по Esc меню выхода (`OKCancelMenu`); здесь Esc отменяет
//!   выбор, как просит постановка.
//! * `GameManager_AutoSelectNext` после подтверждения не вызывается (как и в
//!   остальных приказах порта).

use max_core::access;
use max_core::enums::{UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::{Point, DIRECTION_OFFSETS};
use max_core::production;
use max_core::team::TeamId;
use max_core::unit::{UnitId, UnitInfo};
use max_render::buffer::RgbaBuffer;
use max_render::viewport::Viewport;

use crate::cursor::CursorId;

/// Сдвиг спрайта строителя за один тик `PositionInTape`
/// (`OffsetDrawZones(DIRECTION_OFFSETS[angle] * 8)`, `unitinfo.cpp:4478`).
pub const TAPE_STEP_PIXELS: i32 = 8;
/// Сколько тиков идёт позиционирование (`moved >= 4`).
pub const TAPE_STEPS: u8 = 4;
/// Полный сдвиг в центр ленты: `8 * 4 = 32` px — половина клетки (64 px).
pub const TAPE_CENTER_PIXELS: i32 = TAPE_STEP_PIXELS * TAPE_STEPS as i32;

/// Режим выбора места стройки (`ORDER_STATE_SELECT_SITE`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildSiteSelect {
    /// Строитель (конструктор).
    pub builder: UnitId,
    /// Команда строителя.
    pub team: TeamId,
    /// Тип строителя (`CONSTRCT`).
    pub builder_type: UnitType,
    /// Что строим (голова очереди).
    pub unit_type: UnitType,
    /// Скорость постройки (1/2/4), выбранная в `BuildMenu`.
    pub build_rate: u16,
    /// Якорь 2×2 (клетка ленты `GameManager_TempTape`).
    pub anchor: Point,
    /// Клетка под курсором (для статуса и проверки попадания).
    pub cursor: Point,
    /// Строка статуса (валидна ли клетка).
    pub status: String,
}

impl BuildSiteSelect {
    /// Входит в режим с якорем `anchor` (`UnitsManager_IssueBuildOrder`).
    pub fn new(
        builder: UnitId,
        team: TeamId,
        builder_type: UnitType,
        unit_type: UnitType,
        build_rate: u16,
        builder_grid: Point,
        anchor: Point,
    ) -> Self {
        Self {
            builder,
            team,
            builder_type,
            unit_type,
            build_rate,
            anchor,
            cursor: builder_grid,
            status: String::new(),
        }
    }

    /// Тип временной ленты (`DeployConstructionSiteUtilities`):
    /// здание — `LRGTAPE`, иначе `SMLTAPE`.
    pub fn tape_type(&self) -> UnitType {
        production::construction_utility_types(self.unit_type).0
    }

    /// Инженер (`ENGINEER`) ставит 1×1-площадку — `GameManager_GetBuilderUnitCursor`,
    /// ветка `unit1->GetUnitType() == ENGINEER` (`game_manager.cpp:5854`).
    pub fn is_engineer(&self) -> bool {
        self.builder_type == UnitType::Engineer
    }

    /// Двигает призрак за курсором — `GameManager_GetBuilderUnitCursor`.
    ///
    /// Якорь пересчитывается по каждой оси отдельно и сохраняется, когда
    /// курсор ровно на линии строителя (как C++). У инженера лента 1×1 стоит
    /// прямо в клетке курсора (`target_grid_x = grid_x` в C++).
    pub fn update_cursor(&mut self, state: &GameState, cursor: Point) {
        self.cursor = cursor;

        if let Some(unit) = state.unit(self.builder) {
            self.anchor = production::site_anchor_for_cursor(unit.grid, cursor, self.anchor);
        }

        if self.is_engineer() {
            self.anchor = cursor;
        }

        self.status.clear();
    }

    /// Якорь для произвольной клетки курсора (C++
    /// `GameManager_GetBuilderUnitCursor`): привязка к строителю с сохранением
    /// прежней оси, когда курсор ровно на его линии; у инженера — сама клетка.
    pub fn anchor_at(&self, state: &GameState, cursor: Point) -> Point {
        if self.is_engineer() {
            return cursor;
        }

        match state.unit(self.builder) {
            Some(unit) => production::site_anchor_for_cursor(unit.grid, cursor, self.anchor),
            None => self.anchor,
        }
    }

    /// Валидна ли произвольная клетка курсора при якоре `anchor`.
    pub fn is_valid_at(&self, state: &GameState, anchor: Point, cursor: Point) -> bool {
        let Some(unit) = state.unit(self.builder) else {
            return false;
        };

        if unit.team != self.team {
            return false;
        }

        if self.is_engineer() {
            // `GameManager_GetBuilderUnitCursor` (`game_manager.cpp:5901-5909`):
            // 1×1-лента только в строке/столбце инженера и только на
            // проходимой клетке (`UnitsManager_MoveUnitAndParent`).
            return anchor == cursor
                && production::engineer_site_covers(unit.grid, cursor)
                && production::is_site_accessible_ignoring(
                    state,
                    self.team,
                    self.unit_type,
                    cursor,
                    &[self.builder],
                );
        }

        // C++ `GetBuilderUnitCursor` привязывает 2×2 к строителю: якорь всегда
        // один из четырёх, накрывающих его клетку. Проверка не даёт поставить
        // площадку «вдали» и строить дистанционно, даже если состояние режима
        // пришло из другого места.
        if !production::site_covers(self.unit_type, anchor, unit.grid) {
            return false;
        }

        production::site_covers(self.unit_type, anchor, cursor)
            && production::is_site_accessible_ignoring(
                state,
                self.team,
                self.unit_type,
                anchor,
                &[self.builder],
            )
    }

    /// Валидна ли текущая клетка: курсор внутри 2×2 и площадка проходима
    /// (`UnitsManager_MoveUnitAndParent` в C++).
    pub fn valid(&self, state: &GameState) -> bool {
        self.is_valid_at(state, self.anchor, self.cursor)
    }

    /// Курсор окна: `CURSOR_MAP2` над валидной клеткой, `CURSOR_UNIT_NO_GO`
    /// над невалидной (`GameManager_GetBuilderUnitCursor`).
    pub fn cursor_id(&self, state: &GameState) -> CursorId {
        if self.valid(state) {
            CursorId::Map2
        } else {
            CursorId::UnitNoGo
        }
    }

    /// Текст строки статуса для HUD/лога.
    pub fn status_line(&self, state: &GameState) -> String {
        let (x, y) = (self.anchor.x, self.anchor.y);

        if self.valid(state) {
            format!("Construction site: {x},{y} — LMB builds, Esc/RMB cancels")
        } else {
            format!("Cannot build here ({x},{y}) — move the cursor")
        }
    }

    /// Рисует призрак ленты в кадр (`GameManager_TempTape`).
    ///
    /// Спрайт — тот же, что у настоящей площадки: `LRGTAPE`/`SMLTAPE` с цветами
    /// команды; кадр `0` (лента не анимируется).
    pub fn draw_ghost(
        &self,
        frame: &mut RgbaBuffer,
        assets: &crate::Assets,
        viewport: &Viewport,
    ) -> Option<u32> {
        let tape_type = self.tape_type();
        let mut ghost = UnitInfo::new(tape_type, self.team, UnitId::NO_ID);
        ghost.flags = access::unit_type_flags(tape_type);
        // `IsRelevant` (`unitinfogroup.cpp:59`) рисует только не-`ORDER_IDLE`.
        ghost.orders = UnitOrder::Await;
        ghost.state = UnitOrderState::ExecutingOrder;
        ghost.set_visible_to_team(self.team, true);

        let (world_x, world_y) = crate::render::unit_anchor_world(
            &ghost,
            (f32::from(self.anchor.x), f32::from(self.anchor.y)),
        );
        let (screen_x, screen_y) = viewport.world_to_screen(world_x, world_y);
        // Призрак — не юнит состояния: презентация по умолчанию C++
        // (`brightness = 255`, нули) — Ф5.3/B.
        let presentation = max_core::unit::UnitPresentation::new();
        let drawn = crate::render::draw_unit_sprites(
            assets,
            &ghost,
            &presentation,
            frame,
            (screen_x, screen_y),
            viewport.zoom(),
        )?;

        Some(drawn.unit + drawn.turret)
    }
}

/// Сдвиг спрайта строителя относительно центра его клетки — порт
/// `UnitInfo::PositionInTape` (`unitinfo.cpp:4414`) + `OffsetDrawZones`.
///
/// * `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_{INIT,ENTER}` — `8 px` за тик
///   (`moved` 0..4, как счётчик C++), **по текущему углу корпуса**
///   (`DIRECTION_OFFSETS[angle]`): C++ сначала доворачивает строителя к ленте
///   (`Paths_UpdateAngle`), поэтому спрайт скользит туда, куда смотрит;
/// * `ORDER_BUILD`/`BUILD_IN_PROGRESS` после восстановления приказов —
///   полные `32 px` к центру ленты (рабочая поза уже в центре);
/// * `ORDER_BUILD`/`UNIT_READY` (здание достроено, строитель **ждёт приказа
///   игрока**) — те же `32 px`: C++ не двигает `x`/`y` при смене приказа, поэтому
///   конструктор остаётся в центре площадки, пока его не отправят (`TAPE_
///   POSITIONING_DEINIT` — первый сдвиг);
/// * `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_DEINIT` — строитель ещё в
///   центре ленты (`32 px` въезда), `TAPE_POSITIONING_LEAVE` — те же `32 px`
///   плюс `8 px × moved` в сторону `move_to` (`move_to_grid_*`): так C++
///   съезжает с площадки перед `ActivateEngineer`;
/// * остальные состояния (достройка, `CancelBuilding`, после съезда) — `0`:
///   спрайт в центре своей клетки, как `UnitInfo::Redraw`.
///
/// Направление въезда — угол на центр ленты (у 2×2 это одна из четырёх
/// диагоналей), поэтому сдвиг считается от геометрии площадки, а не от
/// текущего угла корпуса: угол мог быть довёрнут на рабочую позу
/// (`Paths_UpdateAngle(this, (angle + 1) & 0x07)`). Сетка строителя при
/// въезде не меняется.
pub fn tape_center_offset(state: &GameState, unit: &UnitInfo) -> (i32, i32) {
    if unit.unit_type != UnitType::Constructor || !unit.is_mobile() {
        return (0, 0);
    }

    // Лента площадки — та, чей родитель этот строитель
    // (`DeployConstructionSiteUtilities`: `tape->SetParent(this)`).
    let tape = state
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.unit(id))
        .find(|candidate| {
            candidate.parent_unit == Some(unit.id)
                && matches!(
                    candidate.unit_type,
                    UnitType::LargeTape | UnitType::SmallTape
                )
        });

    let Some(tape) = tape else {
        return (0, 0);
    };

    // Центр ленты — только если строитель стоит внутри её 2×2. Иначе сдвиг
    // увёл бы спрайт от центра площадки (старый/повреждённый якорь).
    if !production::site_covers(tape.unit_type, tape.grid, unit.grid) {
        return (0, 0);
    }

    // Въезд: строитель в одной из четырёх клеток 2×2, центр ленты — на 32 px
    // в сторону её центра по каждой оси.
    let (enter_x, enter_y) = if tape.unit_type == UnitType::LargeTape {
        (
            if unit.grid.x == tape.grid.x { 1 } else { -1 },
            if unit.grid.y == tape.grid.y { 1 } else { -1 },
        )
    } else {
        (0, 0)
    };
    let (enter_px_x, enter_px_y) = (enter_x * TAPE_CENTER_PIXELS, enter_y * TAPE_CENTER_PIXELS);

    match (unit.orders, unit.state) {
        (
            UnitOrder::AwaitTapePositioning,
            UnitOrderState::TapePositioningInit | UnitOrderState::TapePositioningEnter,
        ) => {
            let steps = i32::from(unit.moved.min(TAPE_STEPS));
            let direction = DIRECTION_OFFSETS[usize::from(unit.angle & 0x07)];

            (
                i32::from(direction.x) * TAPE_STEP_PIXELS * steps,
                i32::from(direction.y) * TAPE_STEP_PIXELS * steps,
            )
        }
        (UnitOrder::AwaitTapePositioning, UnitOrderState::TapePositioningDeinit) => {
            (enter_px_x, enter_px_y)
        }
        (UnitOrder::AwaitTapePositioning, UnitOrderState::TapePositioningLeave) => {
            let move_to = state.order_data(unit.id).move_to;
            let angle = leave_angle(unit.grid, move_to);
            let direction = DIRECTION_OFFSETS[usize::from(angle & 0x07)];
            let steps = i32::from(unit.moved.min(TAPE_STEPS));

            (
                enter_px_x + i32::from(direction.x) * TAPE_STEP_PIXELS * steps,
                enter_px_y + i32::from(direction.y) * TAPE_STEP_PIXELS * steps,
            )
        }
        (UnitOrder::Build, UnitOrderState::BuildInProgress) => (enter_px_x, enter_px_y),
        // Достройка: C++ `SpawnNewUnit` (`unitinfo.cpp:3440-3592`) переводит
        // строителя в `ORDER_BUILD`/`ORDER_STATE_UNIT_READY`, но его пиксель
        // (`x`/`y`) остаётся там, куда его свёл `PositionInTape` — в центре
        // ленты. Сдвиг не снимается до приказа игрока: первый сброс происходит
        // только в `TAPE_POSITIONING_DEINIT`/`LEAVE`. Раньше здесь был `0`, и
        // спрайт «выталкивало» в угол площадки сразу после достройки.
        (UnitOrder::Build, UnitOrderState::UnitReady) => (enter_px_x, enter_px_y),
        // Клик игрока по клетке: C++ ставит `ORDER_ACTIVATE`/
        // `ORDER_STATE_IN_TRANSITION`, но пиксель строителя ещё в центре ленты.
        (UnitOrder::Activate, UnitOrderState::InTransition) => (enter_px_x, enter_px_y),
        _ => (0, 0),
    }
}

/// `GetTargetUnitAngle` для съезда (`state != TAPE_POSITIONING_ENTER`) —
/// знак разницы до `move_to` по каждой оси; диагонали кодируются углами
/// `NE/SE/SW/NW` ровно как в C++.
fn leave_angle(grid: Point, move_to: Point) -> u8 {
    let target_x = i32::from(move_to.x);
    let target_y = i32::from(move_to.y);

    if target_x > i32::from(grid.x) {
        if target_y > i32::from(grid.y) {
            3
        } else {
            1
        }
    } else if target_y > i32::from(grid.y) {
        5
    } else {
        7
    }
}

/// Мировая точка центра ленты 2×2 с якорем `anchor` (для тестов/ассертов).
pub fn site_center_world(unit_type: UnitType, anchor: Point) -> (i32, i32) {
    let size = i32::from(production::footprint(unit_type));

    (
        i32::from(anchor.x) * 64 + size * 32,
        i32::from(anchor.y) * 64 + size * 32,
    )
}

// ---------------------------------------------------------------------------
// Интеграция с приложением
// ---------------------------------------------------------------------------

impl crate::App {
    /// Открытый режим выбора места стройки; `None` — обычная партия.
    pub fn build_site(&self) -> Option<&BuildSiteSelect> {
        self.build_site.as_ref()
    }

    /// Начинает выбор места (`MobileBuildMenu::Build` → `SELECT_SITE`).
    ///
    /// `false` — рядом со строителем нет проходимого якоря 2×2
    /// (`UnitsManager_IssueBuildOrder` вернул `false`, `BuildMenu` остаётся
    /// открытым с сообщением об ошибке).
    pub fn open_build_site(&mut self, unit_type: UnitType, build_rate: u16) -> bool {
        let Some(menu) = self.build_menu.as_ref() else {
            return false;
        };
        let (builder, team, builder_type) = (menu.unit, menu.team, menu.builder_type);
        let Some(unit) = self.state.unit(builder) else {
            return false;
        };
        let builder_grid = unit.grid;

        let Some(anchor) =
            production::initial_build_site(&self.state, team, unit_type, builder_grid, &[builder])
        else {
            return false;
        };

        self.build_menu = None;
        self.build_site = Some(BuildSiteSelect::new(
            builder,
            team,
            builder_type,
            unit_type,
            build_rate,
            builder_grid,
            anchor,
        ));

        true
    }

    /// Отменяет выбор места (`GameManager_SelectBuildSite`): приказ не уходит.
    pub fn cancel_build_site(&mut self) -> Option<String> {
        let site = self.build_site.take()?;

        Some(format!(
            "Build cancelled: {} not ordered",
            crate::buildmenu::unit_name(site.unit_type)
        ))
    }

    /// Подтверждает выбор места (`GameManager_PathBuild` → `ORDER_BUILD`).
    ///
    /// Возвращает строку журнала; `None` — режима нет. Невалидная клетка
    /// отменяет выбор, как в C++ (`CURSOR_UNIT_NO_GO` → `ManagePlayerAction`).
    pub fn confirm_build_site(&mut self, online: bool) -> Option<String> {
        let (valid, anchor) = {
            let site = self.build_site.as_ref()?;

            (site.valid(&self.state), site.anchor)
        };

        if !valid {
            return self
                .cancel_build_site()
                .map(|line| format!("{line} (invalid cell {},{} )", anchor.x, anchor.y));
        }

        let site = self.build_site.take()?;
        let command = crate::buildmenu::build_order_command(
            &self.state,
            site.builder,
            site.anchor,
            &[site.unit_type],
            false,
            site.build_rate,
        )?;

        if online {
            self.hud.pending.push(command);
        } else {
            self.apply(command);
        }

        Some(format!(
            "Construction site: {} at ({}, {})",
            crate::buildmenu::unit_name(site.unit_type),
            site.anchor.x,
            site.anchor.y
        ))
    }

    /// Закрывает режим без сообщения (выход из партии/загрузка).
    pub fn close_build_site(&mut self) {
        self.build_site = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::unit_flags;
    use max_core::enums::Surface;
    use max_core::geo::{MAP_HEIGHT, MAP_WIDTH};

    fn land_state() -> (GameState, UnitId) {
        let mut state = GameState::new();

        for y in 0..MAP_HEIGHT {
            for x in 0..MAP_WIDTH {
                *state
                    .map
                    .surface
                    .tiles_mut()
                    .get_mut(y as usize * MAP_WIDTH as usize + x as usize)
                    .unwrap() = Surface::LAND;
            }
        }

        state.teams[0].team_type = max_core::team::TeamKind::Player;
        let id = state
            .spawn_unit(
                UnitType::Constructor,
                TeamId::RED,
                Point::new(30, 20),
                unit_flags::MOBILE_LAND_UNIT,
            )
            .expect("constructor spawned");

        (state, id)
    }

    #[test]
    fn anchor_is_pinned_to_the_builder_and_covers_the_cursor() {
        let builder = Point::new(30, 20);

        // Курсор на клетке строителя — якорь не двигается.
        assert_eq!(
            production::site_anchor_for_cursor(builder, builder, Point::new(29, 19)),
            Point::new(29, 19),
            "равенство по обеим осям сохраняет прежний якорь (как C++)"
        );

        // Курсор правее/ниже — якорь = клетка строителя.
        assert_eq!(
            production::site_anchor_for_cursor(builder, Point::new(31, 21), Point::new(0, 0)),
            builder
        );

        // Курсор левее/выше — якорь на клетку влево/вверх.
        assert_eq!(
            production::site_anchor_for_cursor(builder, Point::new(29, 19), Point::new(0, 0)),
            Point::new(29, 19)
        );

        // 2×2 всегда накрывает строителя для любого курсора из этих четырёх.
        for cursor in [(30, 20), (31, 20), (30, 21), (31, 21), (29, 19)] {
            let anchor = production::site_anchor_for_cursor(
                builder,
                Point::new(cursor.0, cursor.1),
                Point::new(29, 19),
            );

            assert!(
                production::site_covers(
                    UnitType::TrainingHall,
                    anchor,
                    Point::new(cursor.0, cursor.1)
                ),
                "курсор {cursor:?} должен попадать в 2×2 с якорем {anchor:?}"
            );
            assert!(
                production::site_covers(UnitType::TrainingHall, anchor, builder),
                "клетка строителя всегда внутри площадки"
            );
        }
    }

    #[test]
    fn initial_site_uses_issue_build_order_candidates() {
        let (state, builder) = land_state();
        let grid = state.unit(builder).unwrap().grid;

        let site = production::initial_build_site(
            &state,
            TeamId::RED,
            UnitType::TrainingHall,
            grid,
            &[builder],
        )
        .expect("площадка рядом со строителем");

        assert_eq!(site, grid, "первый кандидат C++ — клетка строителя");
        assert!(production::site_covers(UnitType::TrainingHall, site, grid));
    }

    #[test]
    fn cursor_selects_the_cell_the_player_points_at() {
        let (mut state, builder) = land_state();
        let grid = state.unit(builder).unwrap().grid;
        let anchor = production::initial_build_site(
            &state,
            TeamId::RED,
            UnitType::TrainingHall,
            grid,
            &[builder],
        )
        .expect("площадка");

        let mut select = BuildSiteSelect::new(
            builder,
            TeamId::RED,
            UnitType::Constructor,
            UnitType::TrainingHall,
            1,
            grid,
            anchor,
        );

        // Курсор на клетке строителя — валидно, `CURSOR_MAP2`.
        assert!(select.valid(&state));
        assert_eq!(select.cursor_id(&state), CursorId::Map2);
        assert_eq!(select.anchor, grid);

        // Курсор влево — якорь уезжает влево вместе с призраком.
        select.update_cursor(&state, Point::new(grid.x - 1, grid.y));
        assert_eq!(select.anchor, Point::new(grid.x - 1, grid.y));
        assert!(select.valid(&state));

        // Вода под площадкой — невалидно, `CURSOR_UNIT_NO_GO`.
        for dy in 0..2 {
            for dx in 0..2 {
                *state
                    .map
                    .surface
                    .tiles_mut()
                    .get_mut(
                        (i32::from(grid.y) + dy) as usize * MAP_WIDTH as usize
                            + (i32::from(grid.x) + dx) as usize,
                    )
                    .unwrap() = Surface::WATER;
            }
        }
        assert!(!select.valid(&state));
        assert_eq!(select.cursor_id(&state), CursorId::UnitNoGo);
    }

    #[test]
    fn anchor_far_from_the_builder_is_never_valid() {
        let (state, builder) = land_state();
        let builder_grid = state.unit(builder).unwrap().grid;
        // Якорь в трёх клетках от строителя — такого не бывает у
        // `GetBuilderUnitCursor`; подтверждение обязано быть невалидным.
        let far = Point::new(builder_grid.x + 3, builder_grid.y);
        let select = BuildSiteSelect::new(
            builder,
            TeamId::RED,
            UnitType::Constructor,
            UnitType::TrainingHall,
            1,
            builder_grid,
            far,
        );

        assert!(!select.is_valid_at(&state, far, far));
        assert!(!select.valid(&state));
        assert_eq!(select.cursor_id(&state), CursorId::UnitNoGo);
    }

    #[test]
    fn builder_does_not_center_on_a_tape_that_does_not_cover_him() {
        let (mut state, builder) = land_state();
        let builder_grid = state.unit(builder).unwrap().grid;
        let far = Point::new(builder_grid.x + 3, builder_grid.y + 3);
        let unit = state.unit_mut(builder).unwrap();
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;

        let _ = max_core::production::deploy_construction_site(
            &mut state,
            builder,
            UnitType::TrainingHall,
            far,
        );

        let unit = state.unit(builder).unwrap();
        assert_eq!(
            tape_center_offset(&state, unit),
            (0, 0),
            "сдвиг к центру только для ленты, накрывающей строителя"
        );
    }

    #[test]
    fn tape_center_offset_moves_the_builder_to_the_tape_center() {
        let (mut state, builder) = land_state();
        // Строитель в правом нижнем углу 2×2, якорь на клетку левее/выше:
        // въезд идёт на NW (−32, −32) — в центр ленты.
        let anchor = Point::new(29, 19);
        state.unit_mut(builder).unwrap().grid = Point::new(30, 20);
        state.unit_mut(builder).unwrap().orders = UnitOrder::Build;
        state.unit_mut(builder).unwrap().state = UnitOrderState::BuildInProgress;
        state.unit_mut(builder).unwrap().moved = 0;
        // C++ при завершении позиционирования доворачивает NW(7) на шаг → N(0).
        state.unit_mut(builder).unwrap().angle = 0;

        let tape = max_core::production::deploy_construction_site(
            &mut state,
            builder,
            UnitType::TrainingHall,
            anchor,
        )
        .expect("площадка поставлена");
        assert_eq!(state.unit(tape.tape).unwrap().parent_unit, Some(builder));

        let unit = state.unit(builder).unwrap();
        assert_eq!(tape_center_offset(&state, unit), (-32, -32));

        let (center_x, center_y) = site_center_world(UnitType::TrainingHall, anchor);
        let (unit_center_x, unit_center_y) = (
            i32::from(unit.grid.x) * 64 + 32 + tape_center_offset(&state, unit).0,
            i32::from(unit.grid.y) * 64 + 32 + tape_center_offset(&state, unit).1,
        );
        assert_eq!(
            (unit_center_x, unit_center_y),
            (center_x, center_y),
            "спрайт строителя встаёт ровно в центр ленты 2×2"
        );

        // Пока идёт позиционирование — сдвиг растёт по 8 px за тик вдоль угла
        // строителя. Угол уже довёрнут к ленте (`Paths_UpdateAngle`), NW = 7.
        state.unit_mut(builder).unwrap().orders = UnitOrder::AwaitTapePositioning;
        state.unit_mut(builder).unwrap().state = UnitOrderState::TapePositioningEnter;
        state.unit_mut(builder).unwrap().angle = 7;
        for moved in 0..=4u8 {
            state.unit_mut(builder).unwrap().moved = moved;
            let unit = state.unit(builder).unwrap();
            let offset = tape_center_offset(&state, unit);
            assert_eq!(offset, (-8 * i32::from(moved), -8 * i32::from(moved)));
        }
    }

    #[test]
    fn tape_center_offset_follows_the_unit_angle_not_a_fixed_diagonal() {
        let (mut state, builder) = land_state();
        let anchor = Point::new(29, 19);
        state.unit_mut(builder).unwrap().grid = Point::new(30, 20);
        state.unit_mut(builder).unwrap().orders = UnitOrder::AwaitTapePositioning;
        state.unit_mut(builder).unwrap().state = UnitOrderState::TapePositioningEnter;
        state.unit_mut(builder).unwrap().moved = 1;

        max_core::production::deploy_construction_site(
            &mut state,
            builder,
            UnitType::TrainingHall,
            anchor,
        )
        .expect("площадка поставлена");

        // Строитель западнее центра, смотрит на восток — сдвиг строго (1,0)*8.
        state.unit_mut(builder).unwrap().angle = 2;
        let unit = state.unit(builder).unwrap();
        assert_eq!(tape_center_offset(&state, unit), (8, 0));

        // Диагональный случай: смотрит NW — (1,1)-диагональ.
        state.unit_mut(builder).unwrap().angle = 7;
        let unit = state.unit(builder).unwrap();
        assert_eq!(tape_center_offset(&state, unit), (-8, -8));

        // Остальные направления — ровно `DIRECTION_OFFSETS[angle] * 8`.
        for angle in 0..8u8 {
            state.unit_mut(builder).unwrap().angle = angle;
            let unit = state.unit(builder).unwrap();
            let offset = tape_center_offset(&state, unit);
            let step = DIRECTION_OFFSETS[usize::from(angle)];

            assert_eq!(
                offset,
                (i32::from(step.x) * 8, i32::from(step.y) * 8),
                "угол {angle}: сдвиг по DIRECTION_OFFSETS"
            );
        }
    }

    #[test]
    fn target_unit_angle_points_from_each_corner_to_the_tape_center() {
        let (mut state, builder) = land_state();
        let anchor = Point::new(29, 19);
        state.unit_mut(builder).unwrap().grid = Point::new(30, 20);
        max_core::production::deploy_construction_site(
            &mut state,
            builder,
            UnitType::TrainingHall,
            anchor,
        )
        .expect("площадка поставлена");

        // `GetTargetUnitAngle`/`TAPE_POSITIONING_ENTER`: четыре угла 2×2 дают
        // четыре диагонали (SE/NE/SW/NW); сетка строителя не меняется.
        let cases = [
            (Point::new(29, 19), 3, (1, 1)),   // NW-угол ленты → SE
            (Point::new(30, 19), 5, (-1, 1)),  // NE-угол → SW
            (Point::new(29, 20), 1, (1, -1)),  // SW-угол → NE
            (Point::new(30, 20), 7, (-1, -1)), // SE-угол → NW
        ];

        for (grid, angle, step) in cases {
            let mut unit = state.unit(builder).unwrap().clone();
            unit.grid = grid;
            unit.state = UnitOrderState::TapePositioningEnter;

            assert_eq!(
                max_core::unit_order::get_target_unit_angle(&state, &unit, unit.target_grid),
                angle,
                "клетка {grid:?}: C++ GetTargetUnitAngle"
            );

            // `DIRECTION_OFFSETS[angle] * 32` из центра клетки — центр ленты.
            let offset = (step.0 * 32, step.1 * 32);
            let (center_x, center_y) = site_center_world(UnitType::TrainingHall, anchor);

            assert_eq!(
                (
                    i32::from(grid.x) * 64 + 32 + offset.0,
                    i32::from(grid.y) * 64 + 32 + offset.1
                ),
                (center_x, center_y),
                "клетка {grid:?}: диагональ ведёт ровно в центр ленты"
            );
        }
    }

    #[test]
    fn tape_center_offset_keeps_the_builder_on_the_tape_while_waiting() {
        let (mut state, builder) = land_state();
        let anchor = Point::new(29, 19);
        state.unit_mut(builder).unwrap().grid = Point::new(30, 20);
        max_core::production::deploy_construction_site(
            &mut state,
            builder,
            UnitType::TrainingHall,
            anchor,
        )
        .expect("площадка поставлена");

        // `ORDER_AWAIT_TAPE_POSITIONING`/`DEINIT` (`PositionInTape`): строитель
        // ещё в центре ленты — сдвиг въезда (строитель в SE-углу 2×2, якорь
        // NW-углом, значит въезд NW: −32,−32).
        let mut unit = state.unit(builder).unwrap().clone();
        unit.orders = UnitOrder::AwaitTapePositioning;
        unit.state = UnitOrderState::TapePositioningDeinit;
        unit.moved = 0;
        assert_eq!(tape_center_offset(&state, &unit), (-32, -32));

        // `ORDER_BUILD`/`BUILD_IN_PROGRESS`: рабочая поза в центре ленты.
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::BuildInProgress;
        assert_eq!(tape_center_offset(&state, &unit), (-32, -32));

        // `ORDER_BUILD`/`UNIT_READY`: здание достроено, строитель ждёт приказа
        // игрока — C++ не двигает его `x`/`y`, значит спрайт остаётся в центре.
        unit.state = UnitOrderState::UnitReady;
        assert_eq!(
            tape_center_offset(&state, &unit),
            (-32, -32),
            "после достройки строитель остаётся в центре ленты"
        );

        // `ORDER_ACTIVATE`/`IN_TRANSITION` — приказ игрока начать уход: C++ ещё
        // не двигал `x`/`y`, поэтому кадр между кликом и тиком обработки остаётся
        // в центре (сдвиг снимается только в `TAPE_POSITIONING_LEAVE`).
        unit.orders = UnitOrder::Activate;
        unit.state = UnitOrderState::InTransition;
        assert_eq!(
            tape_center_offset(&state, &unit),
            (-32, -32),
            "кадр после клика игрока не «прыгает» в угол площадки"
        );

        // `ORDER_ACTIVATE`/`BUILD_IN_PROGRESS` — состояние **после** съезда:
        // `PositionInTape` пересчитал клетку, спрайт снова в центре своей клетки.
        unit.state = UnitOrderState::BuildInProgress;
        assert_eq!(tape_center_offset(&state, &unit), (0, 0));

        // `TAPE_POSITIONING_LEAVE`: те же 32 px плюс 8 px за тик в сторону
        // `move_to` (C++ `move_to_grid_*`): цель западнее-севернее — NW.
        state.set_order_data(
            builder,
            max_core::game::OrderData {
                move_to: Point::new(28, 19),
                fire_on: Point::ZERO,
            },
        );
        unit.state = UnitOrderState::TapePositioningLeave;
        unit.orders = UnitOrder::AwaitTapePositioning;
        for moved in 0..=4u8 {
            unit.moved = moved;
            let step = i32::from(moved);
            assert_eq!(
                tape_center_offset(&state, &unit),
                (-32 - 8 * step, -32 - 8 * step),
                "шаг {moved}: съезд с ленты к выбранной клетке"
            );
        }

        // После четырёх шагов пиксель ровно в центре клетки-якоря (28,19):
        // `PositionInTape` пересчитывает `grid_x = x / 64` в ту же клетку.
        unit.moved = 4;
        let (center_x, center_y) = site_center_world(UnitType::TrainingHall, anchor);
        assert_eq!(
            (
                i32::from(unit.grid.x) * 64 + 32 + tape_center_offset(&state, &unit).0,
                i32::from(unit.grid.y) * 64 + 32 + tape_center_offset(&state, &unit).1,
            ),
            (center_x - 32, center_y - 32),
            "съезд NW на 64 px уводит спрайт на клетку северо-западнее центра ленты"
        );
    }

    #[test]
    fn position_in_tape_turns_to_the_tape_before_the_four_slide_ticks() {
        let (mut state, builder) = land_state();
        let anchor = Point::new(29, 19);
        state.unit_mut(builder).unwrap().grid = Point::new(30, 20);
        max_core::production::deploy_construction_site(
            &mut state,
            builder,
            UnitType::TrainingHall,
            anchor,
        )
        .expect("площадка поставлена");

        let mut unit = state.unit(builder).unwrap().clone();
        unit.orders = UnitOrder::AwaitTapePositioning;
        unit.state = UnitOrderState::TapePositioningInit;
        unit.prior_orders = UnitOrder::Build;
        unit.prior_state = UnitOrderState::BuildInProgress;
        unit.moved = 0;
        unit.angle = 0; // смотрит на север, лента — на северо-запад
        let move_to = unit.target_grid;

        // Тик 1: INIT → ENTER и первый (единственный) шаг поворота 0 → NW(7).
        // C++: `if (moved || !Paths_UpdateAngle(...))` — пока поворот не
        // закончен, `moved` не растёт и спрайт не сдвигается.
        max_core::unit_order::position_in_tape(&mut state, &mut unit, move_to);
        assert_eq!(unit.state, UnitOrderState::TapePositioningEnter);
        assert_eq!(unit.angle, 7, "строитель довернулся к ленте (NW)");
        assert_eq!(unit.moved, 0, "до доворота сдвига нет");
        state.write_unit(builder, unit.clone());
        assert_eq!(tape_center_offset(&state, &unit), (0, 0));

        // Тик 2: угол уже целевой — первый шаг скольжения (−8,−8).
        max_core::unit_order::position_in_tape(&mut state, &mut unit, move_to);
        assert_eq!(unit.moved, 1);
        state.write_unit(builder, unit.clone());
        assert_eq!(tape_center_offset(&state, &unit), (-8, -8));

        // Тики 3–5: ещё три шага по 8 px — ровно в центр ленты.
        for step in 2..=4u8 {
            max_core::unit_order::position_in_tape(&mut state, &mut unit, move_to);
            assert_eq!(unit.moved, step);
            state.write_unit(builder, unit.clone());
            assert_eq!(
                tape_center_offset(&state, &unit),
                (-8 * i32::from(step), -8 * i32::from(step)),
                "шаг {step}: скольжение по диагонали NW"
            );
        }

        let (center_x, center_y) = site_center_world(UnitType::TrainingHall, anchor);
        let offset = tape_center_offset(&state, &unit);
        assert_eq!(
            (
                i32::from(unit.grid.x) * 64 + 32 + offset.0,
                i32::from(unit.grid.y) * 64 + 32 + offset.1
            ),
            (center_x, center_y),
            "четыре шага по 8 px ставят спрайт в центр ленты"
        );

        // Тик 6: `moved >= 4` — `RestoreOrders` в ORDER_BUILD и доворот на шаг
        // (`Paths_UpdateAngle(this, (angle + 1) & 7)`): NW(7) → N(0).
        max_core::unit_order::position_in_tape(&mut state, &mut unit, move_to);
        assert_eq!(unit.orders, UnitOrder::Build);
        assert_eq!(unit.state, UnitOrderState::BuildInProgress);
        assert_eq!(unit.angle, 0, "C++ доворачивает на (angle + 1) & 7");
        assert_eq!(unit.moved, 0);
        state.write_unit(builder, unit.clone());
        assert_eq!(
            tape_center_offset(&state, &unit),
            (-32, -32),
            "рабочая поза остаётся в центре ленты"
        );
    }
}
