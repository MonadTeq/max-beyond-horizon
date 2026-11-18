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
//!    (`UnitInfo::PositionInTape`, `unitinfo.cpp:4414`): `GetTargetUnitAngle`
//!    даёт диагональ к центру 2×2, и четыре тика `OffsetDrawZones` по 8 px
//!    сдвигают **спрайт** строителя на 32 px (половину клетки) — в центр ленты.
//!    Сетка (`grid_x/grid_y`) при этом не меняется.
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
//! * LMB — заказ 08/P4 с `move_to` = якорь призрака; площадку ставит
//!   `unit_order::start_building` на следующем тике, как C++ `StartBuilding`.
//! * RMB/Esc — отмена без заказа (приказ не уходит).
//! * Сдвиг в центр ленты — [`tape_center_offset`] (8 px × 4 тика = 32 px,
//!   `OffsetDrawZones`), применяется рендером к якорю спрайта.
//!
//! # Осознанные отличия
//!
//! * C++ спавнит временную ленту в мире (`GameManager_TempTape`); порт рисует
//!   её как UI-призрак — состояние партии, счётчик юнитов, CRC и сейв не
//!   меняются, пока игрок выбирает место.
//! * `GetBuilderUnitCursor` для `ENGINEER` (лента 1×1, путь по строке/столбцу)
//!   не портирован: инженер ставит 1×1-постройки сразу в своей клетке, как
//!   ветка C++ без `SELECT_SITE`.
//! * C++ открывает по Esc меню выхода (`OKCancelMenu`); здесь Esc отменяет
//!   выбор, как просит постановка.
//! * `GameManager_AutoSelectNext` после подтверждения не вызывается (как и в
//!   остальных приказах порта).

use max_core::access;
use max_core::enums::{unit_flags, UnitOrder, UnitOrderState, UnitType};
use max_core::game::GameState;
use max_core::geo::Point;
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

    /// Двигает призрак за курсором — `GameManager_GetBuilderUnitCursor`.
    ///
    /// Якорь пересчитывается по каждой оси отдельно и сохраняется, когда
    /// курсор ровно на линии строителя (как C++).
    pub fn update_cursor(&mut self, state: &GameState, cursor: Point) {
        self.cursor = cursor;

        if let Some(unit) = state.unit(self.builder) {
            self.anchor = production::site_anchor_for_cursor(unit.grid, cursor, self.anchor);
        }

        self.status.clear();
    }

    /// Якорь 2×2 для произвольной клетки курсора (C++
    /// `GameManager_GetBuilderUnitCursor`): привязка к строителю с сохранением
    /// прежней оси, когда курсор ровно на его линии.
    pub fn anchor_at(&self, state: &GameState, cursor: Point) -> Point {
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
        let drawn = crate::render::draw_unit_sprites(
            assets,
            &ghost,
            frame,
            (screen_x, screen_y),
            viewport.zoom(),
        )?;

        Some(drawn.unit + drawn.turret)
    }
}

/// Сдвиг спрайта строителя к центру ленты — порт `UnitInfo::PositionInTape`
/// (`unitinfo.cpp:4414`) + `OffsetDrawZones`.
///
/// * `AWAIT_TAPE_POSITIONING`/`TAPE_POSITIONING_{INIT,ENTER}` — `8 px` за тик
///   (`moved` 0..4, как счётчик C++);
/// * `ORDER_BUILD`/`BUILD_IN_PROGRESS` после восстановления приказов —
///   полные `32 px` (рабочая поза уже в центре ленты);
/// * остальные состояния (достройка, `CancelBuilding`, уход) — `0`: спрайт
///   возвращается в центр своей клетки, как `UnitInfo::Redraw`.
///
/// Направление — `GetTargetUnitAngle` (`unitinfo.cpp:4147`): по каждой оси
/// знак от клетки строителя к якорю ленты. Сетка строителя не меняется.
pub fn tape_center_offset(state: &GameState, unit: &UnitInfo) -> (i32, i32) {
    if unit.unit_type != UnitType::Constructor || unit.flags & unit_flags::STATIONARY != 0 {
        return (0, 0);
    }

    // Лента площадки — та, чей родитель этот строитель
    // (`DeployConstructionSiteUtilities`: `tape->SetParent(this)`).
    let tape_grid = state
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| state.unit(id))
        .find(|candidate| {
            candidate.parent_unit == Some(unit.id)
                && matches!(
                    candidate.unit_type,
                    UnitType::LargeTape | UnitType::SmallTape
                )
        })
        .map(|tape| tape.grid);

    let Some(tape_grid) = tape_grid else {
        return (0, 0);
    };

    let steps = match (unit.orders, unit.state) {
        (
            UnitOrder::AwaitTapePositioning,
            UnitOrderState::TapePositioningInit | UnitOrderState::TapePositioningEnter,
        ) => i32::from(unit.moved.min(TAPE_STEPS)),
        (UnitOrder::Build, UnitOrderState::BuildInProgress) => i32::from(TAPE_STEPS),
        _ => 0,
    };

    if steps == 0 {
        return (0, 0);
    }

    let direction_x = if unit.grid.x == tape_grid.x { 1 } else { -1 };
    let direction_y = if unit.grid.y == tape_grid.y { 1 } else { -1 };

    (
        direction_x * TAPE_STEP_PIXELS * steps,
        direction_y * TAPE_STEP_PIXELS * steps,
    )
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
    fn tape_center_offset_moves_the_builder_to_the_tape_center() {
        let (mut state, builder) = land_state();
        // Строитель в правом нижнем углу 2×2, якорь на клетку левее/выше:
        // смещение должно быть NW (−32, −32) — в центр ленты.
        let anchor = Point::new(29, 19);
        state.unit_mut(builder).unwrap().grid = Point::new(30, 20);
        state.unit_mut(builder).unwrap().orders = UnitOrder::Build;
        state.unit_mut(builder).unwrap().state = UnitOrderState::BuildInProgress;
        state.unit_mut(builder).unwrap().moved = 0;

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

        // Пока идёт позиционирование — сдвиг растёт по 8 px за тик.
        state.unit_mut(builder).unwrap().orders = UnitOrder::AwaitTapePositioning;
        state.unit_mut(builder).unwrap().state = UnitOrderState::TapePositioningEnter;
        for moved in 0..=4u8 {
            state.unit_mut(builder).unwrap().moved = moved;
            let unit = state.unit(builder).unwrap();
            let offset = tape_center_offset(&state, unit);
            assert_eq!(offset, (-8 * i32::from(moved), -8 * i32::from(moved)));
        }
    }
}
