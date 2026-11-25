use super::*;
use max_core::enums::unit_flags;
use max_core::enums::Surface;
use max_core::geo::{MAP_HEIGHT, MAP_WIDTH};

fn land_state() -> (
    max_core::universe::GameUniverse<max_core::universe::LocalDispatcher>,
    UnitId,
) {
    let mut state = max_core::universe::GameUniverse::default();

    for y in 0..MAP_HEIGHT {
        for x in 0..MAP_WIDTH {
            *state
                .map_mut()
                .surface
                .tiles_mut()
                .get_mut(y as usize * MAP_WIDTH as usize + x as usize)
                .unwrap() = Surface::LAND;
        }
    }

    state
        .team_mut(TeamId::RED)
        .expect("RED slot")
        .team_type = max_core::team::TeamKind::Player;
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

    // Cursor on the builder's cell — the anchor does not move.
    assert_eq!(
        production::site_anchor_for_cursor(builder, builder, Point::new(29, 19)),
        Point::new(29, 19),
            "равенство по обеим осям сохраняет прежний якорь (как C++)"
    );

    // Cursor to the right/below — the anchor = the builder's cell.
    assert_eq!(
        production::site_anchor_for_cursor(builder, Point::new(31, 21), Point::new(0, 0)),
        builder
    );

    // Cursor to the left/above — the anchor one cell left/up.
    assert_eq!(
        production::site_anchor_for_cursor(builder, Point::new(29, 19), Point::new(0, 0)),
        Point::new(29, 19)
    );

    // The 2×2 always covers the builder for any cursor from these four.
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
    let grid = state.snapshot().unit(builder).unwrap().grid;

    let site = StateRef::from(&state).initial_build_site(
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
    let grid = state.snapshot().unit(builder).unwrap().grid;
    let anchor = StateRef::from(&state).initial_build_site(
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

    // Cursor on the builder's cell — valid, `CURSOR_MAP2`.
    assert!(select.valid(StateRef::from(&state)));
    assert_eq!(select.cursor_id(StateRef::from(&state)), CursorId::Map2);
    assert_eq!(select.anchor, grid);

    // Cursor to the left — the anchor moves left together with the ghost.
    select.update_cursor(StateRef::from(&state), Point::new(grid.x - 1, grid.y));
    assert_eq!(select.anchor, Point::new(grid.x - 1, grid.y));
    assert!(select.valid(StateRef::from(&state)));

    // Water under the site — invalid, `CURSOR_UNIT_NO_GO`.
    for dy in 0..2 {
        for dx in 0..2 {
            *state
                .map_mut()
                .surface
                .tiles_mut()
                .get_mut(
                    (i32::from(grid.y) + dy) as usize * MAP_WIDTH as usize
                        + (i32::from(grid.x) + dx) as usize,
                )
                .unwrap() = Surface::WATER;
        }
    }
    assert!(!select.valid(StateRef::from(&state)));
    assert_eq!(select.cursor_id(StateRef::from(&state)), CursorId::UnitNoGo);
}

#[test]
fn anchor_far_from_the_builder_is_never_valid() {
    let (state, builder) = land_state();
    let builder_grid = state.snapshot().unit(builder).unwrap().grid;
    // An anchor three cells away from the builder — that never happens with
    // `GetBuilderUnitCursor`; confirmation must be invalid.
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

    assert!(!select.is_valid_at(StateRef::from(&state), far, far));
    assert!(!select.valid(StateRef::from(&state)));
    assert_eq!(select.cursor_id(StateRef::from(&state)), CursorId::UnitNoGo);
}

#[test]
fn builder_does_not_center_on_a_tape_that_does_not_cover_him() {
    let (mut state, builder) = land_state();
    let builder_grid = state.snapshot().unit(builder).unwrap().grid;
    let far = Point::new(builder_grid.x + 3, builder_grid.y + 3);
    let unit = state.unit_mut(builder).unwrap();
    unit.orders = UnitOrder::Build;
    unit.state = UnitOrderState::BuildInProgress;

    let _ = state.deploy_construction_site(builder,
        UnitType::TrainingHall,
        far,
    );

    let unit = state.snapshot().unit(builder).unwrap();
    assert_eq!(
        tape_center_offset(StateRef::from(&state), unit),
        (0, 0),
            "сдвиг к центру только для ленты, накрывающей строителя"
    );
}

#[test]
fn tape_center_offset_moves_the_builder_to_the_tape_center() {
    let (mut state, builder) = land_state();
    // The builder is in the bottom-right corner of the 2×2, the anchor one cell left/up:
    // the entry goes to NW (−32, −32) — to the tape center.
    let anchor = Point::new(29, 19);
    state.unit_mut(builder).unwrap().grid = Point::new(30, 20);
    state.unit_mut(builder).unwrap().orders = UnitOrder::Build;
    state.unit_mut(builder).unwrap().state = UnitOrderState::BuildInProgress;
    state.unit_mut(builder).unwrap().moved = 0;
    // C++ on finishing the positioning turns NW(7) by a step → N(0).
    state.unit_mut(builder).unwrap().angle = 0;

    let tape = state.deploy_construction_site(builder,
        UnitType::TrainingHall,
        anchor,
    )
    .expect("площадка поставлена");
    assert_eq!(state.snapshot().unit(tape.tape).unwrap().parent_unit, Some(builder));

    let unit = state.snapshot().unit(builder).unwrap();
    assert_eq!(tape_center_offset(StateRef::from(&state), unit), (-32, -32));

    let (center_x, center_y) = site_center_world(UnitType::TrainingHall, anchor);
    let (unit_center_x, unit_center_y) = (
        i32::from(unit.grid.x) * 64 + 32 + tape_center_offset(StateRef::from(&state), unit).0,
        i32::from(unit.grid.y) * 64 + 32 + tape_center_offset(StateRef::from(&state), unit).1,
    );
    assert_eq!(
        (unit_center_x, unit_center_y),
        (center_x, center_y),
            "спрайт строителя встаёт ровно в центр ленты 2×2"
    );

    // While positioning is in progress the shift grows by 8 px per tick along the
    // builder's angle. The angle is already turned toward the tape (`Paths_UpdateAngle`), NW = 7.
    state.unit_mut(builder).unwrap().orders = UnitOrder::AwaitTapePositioning;
    state.unit_mut(builder).unwrap().state = UnitOrderState::TapePositioningEnter;
    state.unit_mut(builder).unwrap().angle = 7;
    for moved in 0..=4u8 {
        state.unit_mut(builder).unwrap().moved = moved;
        let unit = state.snapshot().unit(builder).unwrap();
        let offset = tape_center_offset(StateRef::from(&state), unit);
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

    state.deploy_construction_site(builder,
        UnitType::TrainingHall,
        anchor,
    )
    .expect("площадка поставлена");

    // The builder is west of the center, facing east — the shift is exactly (1,0)*8.
    state.unit_mut(builder).unwrap().angle = 2;
    let unit = state.snapshot().unit(builder).unwrap();
    assert_eq!(tape_center_offset(StateRef::from(&state), unit), (8, 0));

    // Diagonal case: facing NW — the (1,1) diagonal.
    state.unit_mut(builder).unwrap().angle = 7;
    let unit = state.snapshot().unit(builder).unwrap();
    assert_eq!(tape_center_offset(StateRef::from(&state), unit), (-8, -8));

    // The other directions — exactly `DIRECTION_OFFSETS[angle] * 8`.
    for angle in 0..8u8 {
        state.unit_mut(builder).unwrap().angle = angle;
        let unit = state.snapshot().unit(builder).unwrap();
        let offset = tape_center_offset(StateRef::from(&state), unit);
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
    state.deploy_construction_site(builder,
        UnitType::TrainingHall,
        anchor,
    )
    .expect("площадка поставлена");

    // `GetTargetUnitAngle`/`TAPE_POSITIONING_ENTER`: the four corners of the 2×2 give
    // four diagonals (SE/NE/SW/NW); the builder's grid does not change.
    let cases = [
        (Point::new(29, 19), 3, (1, 1)),   // NW corner of the tape → SE
        (Point::new(30, 19), 5, (-1, 1)),  // NE corner → SW
        (Point::new(29, 20), 1, (1, -1)),  // SW corner → NE
        (Point::new(30, 20), 7, (-1, -1)), // SE corner → NW
    ];

    for (grid, angle, step) in cases {
        let mut unit = state.snapshot().unit(builder).unwrap().clone();
        unit.grid = grid;
        unit.state = UnitOrderState::TapePositioningEnter;

        assert_eq!(
            StateRef::from(&state).target_unit_angle( &unit, unit.target_grid),
            angle,
                "клетка {grid:?}: C++ GetTargetUnitAngle"
        );

        // `DIRECTION_OFFSETS[angle] * 32` from the cell center — the tape center.
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
    state.deploy_construction_site(builder,
        UnitType::TrainingHall,
        anchor,
    )
    .expect("площадка поставлена");

    // `ORDER_AWAIT_TAPE_POSITIONING`/`DEINIT` (`PositionInTape`): the builder
    // is still in the tape center — the entry shift (the builder is in the SE corner of the 2×2,
    // the anchor is the NW corner, so the entry is NW: −32,−32).
    let mut unit = state.snapshot().unit(builder).unwrap().clone();
    unit.orders = UnitOrder::AwaitTapePositioning;
    unit.state = UnitOrderState::TapePositioningDeinit;
    unit.moved = 0;
    assert_eq!(tape_center_offset(StateRef::from(&state), &unit), (-32, -32));

    // `ORDER_BUILD`/`BUILD_IN_PROGRESS`: the working pose in the tape center.
    unit.orders = UnitOrder::Build;
    unit.state = UnitOrderState::BuildInProgress;
    assert_eq!(tape_center_offset(StateRef::from(&state), &unit), (-32, -32));

    // `ORDER_BUILD`/`UNIT_READY`: the building is finished, the builder waits for a player
    // order — C++ does not move its `x`/`y`, so the sprite stays in the center.
    unit.state = UnitOrderState::UnitReady;
    assert_eq!(
        tape_center_offset(StateRef::from(&state), &unit),
        (-32, -32),
            "после достройки строитель остаётся в центре ленты"
    );

    // `ORDER_ACTIVATE`/`IN_TRANSITION` — the player's order to start leaving: C++ has not
    // moved `x`/`y` yet, so the frame between the click and the processing tick stays
    // in the center (the shift is removed only in `TAPE_POSITIONING_LEAVE`).
    unit.orders = UnitOrder::Activate;
    unit.state = UnitOrderState::InTransition;
    assert_eq!(
        tape_center_offset(StateRef::from(&state), &unit),
        (-32, -32),
            "кадр после клика игрока не «прыгает» в угол площадки"
    );

    // `ORDER_ACTIVATE`/`BUILD_IN_PROGRESS` — the state **after** leaving:
    // `PositionInTape` recomputed the cell, the sprite is again in the center of its cell.
    unit.state = UnitOrderState::BuildInProgress;
    assert_eq!(tape_center_offset(StateRef::from(&state), &unit), (0, 0));

    // `TAPE_POSITIONING_LEAVE`: the same 32 px plus 8 px per tick toward
    // `move_to` (C++ `move_to_grid_*`): the target is west-north — NW.
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
            tape_center_offset(StateRef::from(&state), &unit),
            (-32 - 8 * step, -32 - 8 * step),
                "шаг {moved}: съезд с ленты к выбранной клетке"
        );
    }

    // After four steps the pixel is exactly in the anchor cell center (28,19):
    // `PositionInTape` recomputes `grid_x = x / 64` into the same cell.
    unit.moved = 4;
    let (center_x, center_y) = site_center_world(UnitType::TrainingHall, anchor);
    assert_eq!(
        (
            i32::from(unit.grid.x) * 64 + 32 + tape_center_offset(StateRef::from(&state), &unit).0,
            i32::from(unit.grid.y) * 64 + 32 + tape_center_offset(StateRef::from(&state), &unit).1,
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
    state.deploy_construction_site(builder,
        UnitType::TrainingHall,
        anchor,
    )
    .expect("площадка поставлена");

    let mut unit = state.snapshot().unit(builder).unwrap().clone();
    unit.orders = UnitOrder::AwaitTapePositioning;
    unit.state = UnitOrderState::TapePositioningInit;
    unit.prior_orders = UnitOrder::Build;
    unit.prior_state = UnitOrderState::BuildInProgress;
    unit.moved = 0;
    unit.angle = 0; // faces north, the tape — north-west
    let move_to = unit.target_grid;

    // Tick 1: INIT → ENTER and the first (only) turn step 0 → NW(7).
    // C++: `if (moved || !Paths_UpdateAngle(...))` — while the turn is not
    // finished, `moved` does not grow and the sprite does not move.
    state.position_in_tape(&mut unit, move_to);
    assert_eq!(unit.state, UnitOrderState::TapePositioningEnter);
    assert_eq!(unit.angle, 7, "строитель довернулся к ленте (NW)");
    assert_eq!(unit.moved, 0, "до доворота сдвига нет");
    state.write_unit(builder, unit.clone());
    assert_eq!(tape_center_offset(StateRef::from(&state), &unit), (0, 0));

    // Tick 2: the angle is already the target — the first sliding step (−8,−8).
    state.position_in_tape(&mut unit, move_to);
    assert_eq!(unit.moved, 1);
    state.write_unit(builder, unit.clone());
    assert_eq!(tape_center_offset(StateRef::from(&state), &unit), (-8, -8));

    // Ticks 3–5: three more steps of 8 px — exactly to the tape center.
    for step in 2..=4u8 {
        state.position_in_tape(&mut unit, move_to);
        assert_eq!(unit.moved, step);
        state.write_unit(builder, unit.clone());
        assert_eq!(
            tape_center_offset(StateRef::from(&state), &unit),
            (-8 * i32::from(step), -8 * i32::from(step)),
                "шаг {step}: скольжение по диагонали NW"
        );
    }

    let (center_x, center_y) = site_center_world(UnitType::TrainingHall, anchor);
    let offset = tape_center_offset(StateRef::from(&state), &unit);
    assert_eq!(
        (
            i32::from(unit.grid.x) * 64 + 32 + offset.0,
            i32::from(unit.grid.y) * 64 + 32 + offset.1
        ),
        (center_x, center_y),
            "четыре шага по 8 px ставят спрайт в центр ленты"
    );

    // Tick 6: `moved >= 4` — `RestoreOrders` into ORDER_BUILD and a turn by a step
    // (`Paths_UpdateAngle(this, (angle + 1) & 7)`): NW(7) → N(0).
    state.position_in_tape(&mut unit, move_to);
    assert_eq!(unit.orders, UnitOrder::Build);
    assert_eq!(unit.state, UnitOrderState::BuildInProgress);
    assert_eq!(unit.angle, 0, "C++ доворачивает на (angle + 1) & 7");
    assert_eq!(unit.moved, 0);
    state.write_unit(builder, unit.clone());
    assert_eq!(
        tape_center_offset(StateRef::from(&state), &unit),
        (-32, -32),
            "рабочая поза остаётся в центре ленты"
    );
}
