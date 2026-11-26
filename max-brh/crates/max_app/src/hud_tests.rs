use super::*;

use max_core::enums::{GameStateType, TeamClan, UnitType};
use max_core::input::Packet18Chat;
use max_core::team::{TeamKind, TeamUnits};
use max_net::Packet;

use crate::ui::{handle_session_input, InputEvent, Key};
use crate::{App, AppConfig, Transition};

/// Application with a RED tank (10,10), a BLUE tank (14,10) and team gold.
fn test_app() -> (App, UnitId, UnitId) {
    let mut app = App::new(AppConfig::default().with_screen(512, 384));

    let (tank, enemy) = {
        let universe = &mut app.universe;
        universe.set_player_team(TeamId::RED);
        if let Some(info) = universe.team_mut(TeamId::RED) {
            info.team_type = TeamKind::Player;
        }
        if let Some(info) = universe.team_mut(TeamId::BLUE) {
            info.team_type = TeamKind::Computer;
        }
        // Unit attributes are taken from the team `current_values` (as in
        // `GameState::with_setup`), otherwise `hits`/`ammo` would be zero.
        if let Some(units) = universe.team_units_mut(TeamId::RED) {
            *units = TeamUnits::with_clan(TeamClan::Random);
            units.set_gold(1500);
        }
        if let Some(units) = universe.team_units_mut(TeamId::BLUE) {
            *units = TeamUnits::with_clan(TeamClan::Random);
        }

        let tank = universe
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
            .expect("tank spawned");
        let enemy = universe
            .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(14, 10), 0)
            .expect("enemy spawned");

        (tank, enemy)
    };

    // Camera on the middle between the units: both cells in the 512×384 frame.
    app.viewport_mut().center_on_grid(12, 10);

    (app, tank, enemy)
}

/// Screen point of the cell center.
fn screen_of<D: Dispatcher>(app: &App<D>, grid: Point) -> (i32, i32) {
    tile_center(app.viewport(), grid)
}

/// The same world as [`test_app`], on a lockstep application.
///
/// SDL/UI Stage C2: the immediate/queued behaviour is the dispatcher
/// capability, not an argument, so the queued-order tests build an
/// `App<NetworkDispatcher>` (`SessionChannel::Lockstep`) instead of passing
/// `online = true` to [`handle_session_input`].
fn network_test_app() -> (App<crate::net::NetworkDispatcher>, UnitId, UnitId) {
    let mut app = App::<crate::net::NetworkDispatcher>::new_network(
        AppConfig::default().with_screen(512, 384),
        TeamId::RED,
        max_core::enums::PlayMode::TurnBased,
    );

    let (tank, enemy) = {
        let universe = &mut app.universe;
        if let Some(info) = universe.team_mut(TeamId::RED) {
            info.team_type = TeamKind::Player;
        }
        if let Some(info) = universe.team_mut(TeamId::BLUE) {
            info.team_type = TeamKind::Computer;
        }
        if let Some(units) = universe.team_units_mut(TeamId::RED) {
            *units = TeamUnits::with_clan(TeamClan::Random);
            units.set_gold(1500);
        }
        if let Some(units) = universe.team_units_mut(TeamId::BLUE) {
            *units = TeamUnits::with_clan(TeamClan::Random);
        }

        let tank = universe
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
            .expect("tank spawned");
        let enemy = universe
            .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(14, 10), 0)
            .expect("enemy spawned");

        (tank, enemy)
    };

    app.viewport_mut().center_on_grid(12, 10);

    (app, tank, enemy)
}

#[test]
fn message_log_is_a_ring_buffer() {
    let mut log = MessageLog::with_capacity(3);

    for index in 0..5 {
        log.push(format!("line {index}"));
    }

    assert_eq!(log.capacity(), 3);
    assert_eq!(log.len(), 3);
    assert_eq!(log.lines(), ["line 2", "line 3", "line 4"]);

    log.clear();
    assert!(log.is_empty());
    // Capacity 0 is treated as 1 — the log does not lose the last line.
    let mut one = MessageLog::with_capacity(0);
    one.push("a");
    one.push("b");
    assert_eq!(one.lines(), ["b"]);
}

#[test]
fn ground_cover_slab_under_a_building_does_not_steal_the_click() {
    // C++ `Access_GetTeamUnit` (`access.cpp:1804`) first looks for
    // `SELECTABLE` and not `GROUND_COVER`; the `LRGSLAB` slab lies under every
    // building, but the click must go to the building. The port earlier took the first
    // unit in `Hash_MapHash`, and after construction the slab was inserted before
    // the building — `LargeSlab` was selected.
    let (mut app, _tank, _enemy) = test_app();
    let grid = Point::new(11, 10);

    let slab = app
        .universe
        .spawn_unit(UnitType::LargeSlab, TeamId::RED, grid, 0)
        .expect("slab");
    let building = app
        .universe
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, grid, 0)
        .expect("building");

    let (x, y) = screen_of(&app, grid);
    let picked = unit_at_screen(app.state_ref(), app.viewport(), TeamId::RED, x, y);

    assert_eq!(
        picked,
        Some(building),
            "клик должен выбрать здание, не плиту"
    );
    assert_ne!(picked, Some(slab));
}

#[test]
fn hud_renders_on_empty_state_without_panics() {
    // Empty match: no units, no teams, a minimal frame and a normal one.
    for (width, height) in [(320, 240), (512, 384), (64, 48)] {
        let app = App::new(AppConfig::default().with_screen(width, height));
        let mut buffer = RgbaBuffer::new(width, height);

        render_hud(
            &mut buffer,
            &HudView {
                state: app.state_ref(),
                hud: app.hud(),
                viewport: app.viewport(),
                team: app.player_team(),
                status: "empty",
            },
        );

        assert_eq!(buffer.width(), width);
        assert!(
            buffer.pixels().chunks_exact(4).any(|pixel| pixel[3] == 255),
                "HUD не нарисовал ни одного пикселя на {width}x{height}"
        );
    }
}

#[test]
fn minimap_draws_surface_colors_and_own_units() {
    let (app, tank, _enemy) = test_app();
    let layout = hud_layout(512, 384);
    let mut buffer = RgbaBuffer::new(512, 384);

    draw_minimap(
        &mut buffer,
        layout.minimap,
        app.state_ref(),
        app.viewport(),
        app.player_team(),
        Some(tank),
    );

    // A minimap cell inside the map — the surface color (LAND), not transparent.
    let center = (
        layout.minimap.ulx + layout.minimap.width / 2,
        layout.minimap.uly + layout.minimap.height / 2,
    );
    let pixel = buffer.get(center.0, center.1).expect("pixel in minimap");

    assert_eq!(pixel.a, 255);
    assert_ne!(pixel, Rgba::TRANSPARENT);

    // There are pixels of the team color (the unit dot, the selected one — 3×3).
    let team_pixels = buffer
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[..3] == team_color(TeamId::RED).to_rgba()[..3])
        .count();

    assert!(
        team_pixels >= 9,
            "точка выделенного юнита должна быть видна на миникарте (найдено {team_pixels})"
    );
}

#[test]
fn minimap_click_maps_to_a_tile_and_moves_the_camera() {
    let layout = hud_layout(512, 384);
    let point = (
        layout.minimap.ulx + layout.minimap.width / 2,
        layout.minimap.uly + layout.minimap.height / 2,
    );
    let grid = minimap_grid_at(&layout, point.0, point.1).expect("grid under minimap");

    assert!((50..=60).contains(&grid.x), "центр карты по x: {grid:?}");
    assert!((50..=60).contains(&grid.y), "центр карты по y: {grid:?}");
    // Outside the minimap — None.
    assert_eq!(minimap_grid_at(&layout, -5, -5), None);
}

#[test]
fn hud_buttons_are_hit_testable() {
    let (width, height) = (512, 384);
    let layout = hud_layout(width, height);

    for button in HudButton::ALL {
        let rect = layout.button(button);
        let point = (rect.x + rect.width / 2, rect.y + rect.height / 2);

        assert_eq!(hud_button_at(width, height, point.0, point.1), Some(button));
    }

    // The buttons do not overlap and lie inside the frame.
    assert!(layout.end_turn.bottom() <= layout.menu.y);
    assert!(layout.menu.bottom() <= height as i32);
    // The map center is not a button.
    assert_eq!(hud_button_at(width, height, 100, 100), None);
}

#[test]
fn click_selects_own_unit_and_enemy_click_orders_attack() {
    let (mut app, tank, enemy) = test_app();

    let (x, y) = screen_of(&app, Point::new(10, 10));
    let transition = handle_session_input(&mut app, InputEvent::MouseClick(x, y));

    assert_eq!(transition, Transition::None);
    assert_eq!(app.hud().selected, Some(tank));
    assert!(app
        .hud()
        .log
        .lines()
        .iter()
        .any(|line| line.contains("Selected")));

    // A click on an enemy with an own unit selected — an attack order (08).
    let (ex, ey) = screen_of(&app, Point::new(14, 10));
    handle_session_input(&mut app, InputEvent::MouseClick(ex, ey));

    // Demo mode applied the packet locally via `apply_command`.
    let unit = app.state_ref().unit(tank).expect("tank");

    assert_eq!(unit.orders, UnitOrder::MoveToAttack);
    assert_eq!(unit.enemy_unit, Some(enemy));
    assert_eq!(app.hud().selected, Some(tank));
    assert!(app.hud().pending.is_empty(), "демо-режим не копит приказы");
    // The path to the attack position will be requested by the unit itself on the tick
    // (`process_order_move` → `Paths_RequestPath`), packet 08 does not carry it.
    assert!(app.state_ref().path_requests().is_empty());
}

#[test]
fn right_click_selects_a_unit_and_shows_stats_without_a_menu() {
    let (mut app, tank, _enemy) = test_app();
    let (x, y) = screen_of(&app, Point::new(10, 10));

    // Oracle `game_manager.cpp:6747-6760`: RMB on an own unit is
    // `GameManager_UnitSelect` — selection only, the command menu stays closed
    // (`GameManager_ClickUnit` opens it on the repeated LMB).
    handle_session_input(&mut app, InputEvent::RightClick(x, y));

    assert_eq!(app.hud().selected, Some(tank));
    assert!(app.unit_menu().is_none(), "RMB must not open the command menu");
    assert!(app
        .hud()
        .log
        .lines()
        .iter()
        .any(|line| line.contains("Selected")));

    // A repeated RMB on the selected unit — `UnitStats_Menu` (`:6751-6753`):
    // the full-screen `STATS` window opens.
    handle_session_input(&mut app, InputEvent::RightClick(x, y));

    assert_eq!(app.hud().selected, Some(tank));
    assert!(
        app.unit_stats().is_some(),
        "RMB on the selected unit opens the statistics window"
    );
    assert!(app
        .hud()
        .log
        .lines()
        .iter()
        .any(|line| line.contains("Unit stats")));

    // `Esc` closes the window (`UnitStatsMenu::Run`) and returns to the game.
    handle_session_input(&mut app, InputEvent::Key(Key::Escape));
    assert!(app.unit_stats().is_none(), "Esc closes the statistics window");
    assert_eq!(app.hud().selected, Some(tank));
}

#[test]
fn right_click_on_a_free_cell_deselects_and_cancels_the_modes() {
    let (mut app, tank, _enemy) = test_app();
    let (x, y) = screen_of(&app, Point::new(10, 10));

    handle_session_input(&mut app, InputEvent::MouseClick(x, y));
    app.hud_mut().targeting_mode = true;
    app.hud_mut().enter_mode = true;
    app.hud_mut().cursor_mode = crate::unitmenu::CURSOR_LOAD;

    let (ex, ey) = screen_of(&app, Point::new(12, 10));
    handle_session_input(&mut app, InputEvent::RightClick(ex, ey));

    // `targeting_mode = 0`, `enter_mode = 0`, `cursor = CURSOR_HIDDEN`
    // (`game_manager.cpp:6764-6770`) and `GameManager_MenuUnitSelect(nullptr)`
    // (`game_manager.cpp:7247-7253`): `GameManager_SelectedUnit = nullptr`.
    assert_eq!(
        app.hud().selected,
        None,
        "the empty cell under the right button drops the focus"
    );
    assert_ne!(app.hud().selected, Some(tank));
    assert!(!app.hud().targeting_mode);
    assert!(!app.hud().enter_mode);
    assert_eq!(app.hud().cursor_mode, 0);
    assert!(app.portrait().is_none(), "the corner FLC is freed");
    assert!(app
        .hud()
        .log
        .lines()
        .iter()
        .any(|line| line.contains("Cancelled")));
}

#[test]
fn right_click_on_a_free_cell_deselects_a_building() {
    let (mut app, _tank, _enemy) = test_app();

    // A stationary building of the same team, two cells below the tank.
    let building = app
        .universe
        .spawn_unit(
            UnitType::MiningStation,
            TeamId::RED,
            Point::new(10, 12),
            max_core::access::unit_type_flags(UnitType::MiningStation),
        )
        .expect("mine spawned");

    let (bx, by) = screen_of(&app, Point::new(10, 12));
    handle_session_input(&mut app, InputEvent::MouseClick(bx, by));
    assert_eq!(app.hud().selected, Some(building));

    // A free cell next to it.
    let (ex, ey) = screen_of(&app, Point::new(12, 12));
    assert!(app.state_ref().units_at(Point::new(12, 12)).is_empty());
    handle_session_input(&mut app, InputEvent::RightClick(ex, ey));

    assert_eq!(
        app.hud().selected,
        None,
        "RMB on an empty cell releases a building as well"
    );
}

#[test]
fn left_click_on_a_free_cell_deselects_nothing_without_a_unit() {
    let (mut app, _tank, _enemy) = test_app();
    let (x, y) = screen_of(&app, Point::new(12, 10));

    handle_session_input(&mut app, InputEvent::MouseClick(x, y));

    // C++ `CURSOR_UNIT_NO_GO` with no selection does nothing — there is no
    // "deselect on empty click" (`game_manager.cpp:6930-6943`).
    assert_eq!(app.hud().selected, None);
    assert!(app.hud().pending.is_empty());
}

#[test]
fn path_preview_builds_a_route_and_draws_a_line() {
    let (app, tank, _enemy) = test_app();
    let destination = Point::new(13, 10);

    let order = build_move_order(app.state_ref(), tank, destination).expect("route");

    assert_eq!(order.preview.unit, tank);
    assert_eq!(order.preview.from, Point::new(10, 10));
    assert_eq!(order.preview.to, destination);
    assert_eq!(order.preview.len(), order.preview.steps.len());
    assert!(order.preview.len() >= 3, "3 клетки — минимум 3 шага");
    // The end of the actual route coincides with the target.
    assert_eq!(order.preview.last_point(), destination);

    // The line is drawn: before rendering the frame is transparent, after — there are pixels of the
    // preview color.
    let mut buffer = RgbaBuffer::new(512, 384);
    draw_path_preview(&mut buffer, app.viewport(), &order.preview);

    let painted = buffer
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| pixel[..3] == PREVIEW_COLOR.to_rgba()[..3])
        .count();

    assert!(
        painted > 50,
            "линия маршрута должна быть видна ({painted} px)"
    );
}

#[test]
fn move_order_packet_38_matches_the_wire_layout() {
    let (app, tank, _enemy) = test_app();
    // Adjacent cell: a one-step path, the bytes are fully predictable.
    let order = build_move_order(app.state_ref(), tank, Point::new(11, 10)).expect("route");
    let bytes = Packet::Command(order.command.clone())
        .encode()
        .expect("38 encoded");

    let low = tank.0 as u8;
    let high = (tank.0 >> 8) as u8;
    let expected = [
        0x76, low, high, // type 0x26|APPL, unit_id
        0x02, 0x05, // order Move, state InProgress (the path is ready)
        0x0A, 0x00, 0x0A, 0x00, // grid (10,10)
        0x0B, 0x00, 0x0A, 0x00, // move_to (11,10)
        0x00, 0x00, // group_speed
        0x01, 0x00, // steps_count
        0x01, 0x00, // PathStep (1,0)
    ];

    assert_eq!(bytes, expected);

    // Decodes back into the same `Command` (checked against the wire encoder).
    let decoded = Packet::decode(&bytes).expect("38 decoded");

    assert_eq!(decoded, Packet::Command(order.command));
}

#[test]
fn attack_order_packet_08_matches_the_wire_layout() {
    let (app, tank, enemy) = test_app();
    let command = build_attack_order(app.state_ref(), tank, enemy).expect("attack order");
    let bytes = Packet::Command(command.clone())
        .encode()
        .expect("08 encoded");

    let low = tank.0 as u8;
    let high = (tank.0 >> 8) as u8;
    let enemy_low = enemy.0 as u8;
    let enemy_high = (enemy.0 >> 8) as u8;
    let expected = [
        0x58, low, high, // type 0x08|APPL, unit_id
        // MoveToAttack/Init; prior — `Await`/`ExecutingOrder`: the new unit
        // is born in `ORDER_AWAIT`/`ORDER_STATE_EXECUTING_ORDER`
        // (`unitinfo.cpp:201-204`), and `SetNewOrderInt` saves it in prior
        // (`units_manager.cpp:1970-1971`).
        0x1E, 0x00, 0x00, 0x01, 0x00, // no disabled
        0x00, 0x00, 0x00, 0x00, // parent_unit, experience
        0x0E, 0x00, 0x0A, 0x00, // move_to (14,10) — the enemy cell
        0x0E, 0x00, 0x0A, 0x00, // fire_on (14,10)
        enemy_low, enemy_high, // enemy_unit
        0x00, 0x00, // transfer_cargo
        0x00, // stealth_dice_roll
    ];

    assert_eq!(bytes, expected);
    assert_eq!(
        Packet::decode(&bytes).expect("08 decoded"),
        Packet::Command(command)
    );
}

#[test]
fn left_click_on_a_free_cell_queues_a_move_in_a_lockstep_session() {
    let (mut app, tank, _enemy) = network_test_app();
    let (x, y) = screen_of(&app, Point::new(10, 10));

    handle_session_input(&mut app, InputEvent::MouseClick(x, y));

    let (mx, my) = screen_of(&app, Point::new(11, 10));
    // Oracle `game_manager.cpp:6685-6745`/`:6894-6902`: LMB (`CURSOR_UNIT_GO`)
    // is the move button.
    handle_session_input(&mut app, InputEvent::MouseClick(mx, my));

    assert_eq!(
        app.hud().pending.len(),
        1,
            "the lockstep session queues the order for the session driver"
    );
    assert!(matches!(&app.hud().pending[0], Command::Move(_)));
    assert_eq!(
        app.hud().preview.as_ref().map(|preview| preview.to),
        Some(Point::new(11, 10))
    );
    // Not applied in the producing frame — the session driver submits it and the
    // simulation applies it on the next tick.
    assert_eq!(
        app.state_ref().unit(tank).expect("tank").orders,
        UnitOrder::Await
    );

    // We take them: the session would send this to `SessionHandle`.
    let outgoing = app.take_outgoing_orders();
    assert_eq!(outgoing.len(), 1);
    assert!(app.hud().pending.is_empty());
}

#[test]
fn demo_orders_apply_through_apply_command() {
    let (mut app, tank, _enemy) = test_app();
    let (x, y) = screen_of(&app, Point::new(10, 10));

    handle_session_input(&mut app, InputEvent::MouseClick(x, y));

    let (mx, my) = screen_of(&app, Point::new(13, 10));
    handle_session_input(&mut app, InputEvent::MouseClick(mx, my));

    let unit = app.state_ref().unit(tank).expect("tank");

    assert_eq!(unit.orders, UnitOrder::Move);
    assert_eq!(unit.state, UnitOrderState::InProgress);
    assert!(matches!(
        &unit.path,
        Some(max_core::unit::UnitPath::Ground(_))
    ));
    assert_eq!(unit.target_grid, Point::new(13, 10));
    assert_eq!(app.applied_commands(), 1);
    assert!(app
        .hud()
        .log
        .lines()
        .iter()
        .any(|line| line.contains("Move")));
}

/// Oracle `game_manager.cpp:6685-6745` + `:6246-6263` (`GameManager_ClickUnit`):
/// LMB on an own unit selects it, a repeated LMB opens the command menu. A click
/// on a **ground cover** unit (road/bridge/water platform) is a move onto that
/// cell (`CURSOR_UNIT_GO`, `GetUnitActionCursor`), not a selection of the cover.
#[test]
fn repeated_left_click_opens_the_menu_and_ground_cover_is_clicked_through() {
    let (mut app, tank, _enemy) = test_app();

    // A road on the target cell; a tank selected and an LMB on the road cell.
    let road = app
        .universe
        .spawn_unit(UnitType::Road, TeamId::RED, Point::new(12, 10), 0)
        .expect("road");

    let (x, y) = screen_of(&app, Point::new(10, 10));
    handle_session_input(&mut app, InputEvent::MouseClick(x, y));
    assert_eq!(app.hud().selected, Some(tank));

    let (rx, ry) = screen_of(&app, Point::new(12, 10));
    handle_session_input(&mut app, InputEvent::MouseClick(rx, ry));

    // The click moved the tank instead of selecting the road.
    assert_eq!(app.hud().selected, Some(tank));
    let unit = app.state_ref().unit(tank).expect("tank");
    assert_eq!(unit.orders, UnitOrder::Move);
    assert_eq!(unit.target_grid, Point::new(12, 10));
    assert!(app.state_ref().unit(road).is_some());

    // A repeated LMB on the selected unit opens its command menu.
    let (ux, uy) = screen_of(&app, Point::new(10, 10));
    handle_session_input(&mut app, InputEvent::MouseClick(ux, uy));

    assert!(app.unit_menu().is_some(), "the repeated LMB opens the popup");
}

/// Oracle `GameManager_GetAirUnitCursor` (`game_manager.cpp:5943-5960`): with an
/// aircraft selected, an own landing pad is the one own unit the cursor treats as
/// `CURSOR_UNIT_GO` (`team_unit->GetUnitType() != LANDPAD`), so LMB orders a move
/// to its cell — the aircraft lands on arrival (`UnitsManager_LoadUnit` →
/// `ORDER_LAND`). Before the click-through the pad was merely selected and
/// aircraft could not be parked.
#[test]
fn aircraft_left_click_on_a_landing_pad_orders_a_landing() {
    let mut app = App::new(AppConfig::default().with_screen(512, 384));

    {
        let universe = &mut app.universe;
        universe.set_player_team(TeamId::RED);

        if let Some(info) = universe.team_mut(TeamId::RED) {
            info.team_type = TeamKind::Player;
        }
        if let Some(units) = universe.team_units_mut(TeamId::RED) {
            *units = TeamUnits::with_clan(TeamClan::Random);
        }
    }

    let pad = app
        .universe
        .spawn_unit(UnitType::LandingPad, TeamId::RED, Point::new(10, 10), 0)
        .expect("landing pad");
    let plane = app
        .universe
        .spawn_unit(UnitType::Fighter, TeamId::RED, Point::new(10, 13), 0)
        .expect("aircraft");

    app.viewport_mut().center_on_grid(10, 11);

    let (px, py) = screen_of(&app, Point::new(10, 13));
    handle_session_input(&mut app, InputEvent::MouseClick(px, py));
    assert_eq!(app.hud().selected, Some(plane));

    let (lx, ly) = screen_of(&app, Point::new(10, 10));
    handle_session_input(&mut app, InputEvent::MouseClick(lx, ly));

    assert_eq!(app.hud().selected, Some(plane), "the pad must not be selected");

    let unit = app.state_ref().unit(plane).expect("aircraft");
    assert_eq!(unit.orders, UnitOrder::Move, "the click is an air move order");
    assert_eq!(unit.target_grid, Point::new(10, 10), "destination is the pad");
    assert!(app.state_ref().unit(pad).is_some());
}

/// SDL/UI Stage B, B1: the demo End Turn is the facade's local-machine
/// operation [`max_core::universe::GameUniverse::end_local_turn`] — the same
/// one a local match uses (`06` immediately, `enter_end_turn`, the team-turn
/// machine), not the former `set_game_state` + `finish_turn` pair.
///
/// The round cursor of this fixture was never entered through
/// `begin_local_round`, so the machine restarts RED's turn instead of closing
/// the round: the turn counter does not advance and the match continues. This
/// is the intended B1 behavior (owner decision 1) — the demo no longer closes
/// a whole round with one End Turn.
#[test]
fn end_turn_closes_the_turn_through_the_local_machine() {
    let (mut app, _tank, _enemy) = test_app();
    let before = app.state_ref().turn_counter();

    let over = app.end_turn();

    // `06` is applied inside the facade (`end_local_turn`), so the application's
    // own counter does not grow; the machine starts the next acting team.
    assert!(!over, "дуэль из двух играющих команд продолжается");
    assert_eq!(app.applied_commands(), 0);
    assert!(app
        .hud()
        .log
        .lines()
        .iter()
        .any(|line| line.contains("End Turn")));
    assert_eq!(app.state_ref().game_state(), GameStateType::InGame);
    assert_eq!(app.state_ref().active_turn_team(), TeamId::RED);
    assert_eq!(app.state_ref().turn_counter(), before);
    assert!(
        !app.state_ref().teams()[TeamId::RED.0 as usize].finished_turn,
            "the machine resets `finished_turn` when it starts the team"
    );
}

#[test]
fn resources_and_unit_panel_show_the_documented_fields() {
    let (app, tank, _enemy) = test_app();
    let line = resources_line(app.state_ref(), TeamId::RED);

    assert!(line.contains("GOLD 1500"), "{line}");
    assert!(line.contains("TEAM 0"), "{line}");

    let lines = unit_panel_lines(app.state_ref(), tank);

    assert_eq!(lines.len(), 5);
    assert!(lines[0].contains("Tank"), "{lines:?}");
    assert!(lines[1].starts_with("HP "), "{lines:?}");
    assert!(lines[2].starts_with("AMMO "), "{lines:?}");
    assert!(lines[3].contains("ORDER Await"), "{lines:?}");
    assert!(lines[4].contains("GRID 10,10"), "{lines:?}");
}

/// The unit panel (bottom lines, not `STAT_WINDOW`) does not show Speed —
/// neither for a tank nor for a building (in C++ `Speed` is not in this panel at all).
#[test]
fn unit_panel_lines_never_show_speed() {
    let (mut app, tank, _enemy) = test_app();
    let mine = app
        .universe
        .spawn_unit(UnitType::MiningStation, TeamId::RED, Point::new(16, 10), 0)
        .expect("шахта создана");

    for (id, what) in [(tank, "танк"), (mine, "шахта")] {
        let lines = unit_panel_lines(app.state_ref(), id);

        assert!(
            lines
                .iter()
                .all(|line| !line.contains("SPEED") && !line.contains("Speed")),
                "{what}: {lines:?}"
        );
    }
}

#[test]
fn escape_cancels_then_leaves_the_session() {
    let (mut app, tank, _enemy) = test_app();
    let (x, y) = screen_of(&app, Point::new(10, 10));

    handle_session_input(&mut app, InputEvent::MouseClick(x, y));
    assert_eq!(app.hud().selected, Some(tank));

    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape)),
        Transition::None
    );
    assert_eq!(app.hud().selected, None);
    // A second Esc — exit the session.
    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape)),
        Transition::LeaveSession
    );
}

#[test]
fn selection_stays_out_of_the_snapshot() {
    let (mut app, tank, _enemy) = test_app();
    let before = app.snapshot();

    app.select_unit(Some(tank));
    app.hud_mut().push_log("hello");
    app.hud_mut().hover_button = Some(HudButton::EndTurn);

    assert_eq!(app.snapshot(), before, "HUD не входит в снапшот/CRC");
}

#[test]
fn chat_input_is_ascii_only_with_backspace_and_a_limit() {
    let mut chat = ChatInput::new();

    assert!(!chat.push_char('A'), "закрытый ввод не принимает символы");
    chat.open();
    assert!(chat.is_active());
    assert!(chat.push_char('H'));
    assert!(chat.push_char('i'));
    assert!(chat.push_char(' '));
    assert!(!chat.push_char('Ж'), "только ASCII");
    assert!(!chat.push_char('\n'), "служебные символы не печатаются");
    assert_eq!(chat.message(), "Hi ");

    assert!(chat.backspace());
    assert_eq!(chat.message(), "Hi");

    for _ in 0..(CHAT_MAX_CHARS + 10) {
        chat.push_char('x');
    }
    assert_eq!(chat.message().chars().count(), CHAT_MAX_CHARS);

    chat.close();
    assert!(!chat.is_active());
    assert_eq!(chat.message(), "");

    // An empty Enter closes the input and returns nothing.
    chat.open();
    assert_eq!(chat.take_message(), None);
    assert!(!chat.is_active());
}

#[test]
fn chat_overlay_draws_the_prompt_above_the_hud_bar() {
    let (mut app, _tank, _enemy) = test_app();
    let layout = hud_layout(512, 384);
    let rect = chat_input_rect(512, 384, &layout);

    assert!(!rect.is_empty());
    assert!(rect.uly >= 0 && rect.lry() <= 384, "{rect:?}");
    assert!(
        rect.uly <= layout.bar.uly,
            "строка ввода выше нижней панели"
    );

    let view = HudView {
        state: app.state_ref(),
        hud: app.hud(),
        viewport: app.viewport(),
        team: app.player_team(),
        status: "",
    };
    let mut closed = RgbaBuffer::new(512, 384);
    render_hud(&mut closed, &view);
    let before = closed.get(rect.ulx + 2, rect.uly + 2).expect("pixel");

    app.hud_mut().chat.open();
    app.hud_mut().chat.push_char('h');
    let view = HudView {
        state: app.state_ref(),
        hud: app.hud(),
        viewport: app.viewport(),
        team: app.player_team(),
        status: "",
    };
    let mut open = RgbaBuffer::new(512, 384);
    render_hud(&mut open, &view);
    let after = open.get(rect.ulx + 2, rect.uly + 2).expect("pixel");

    assert_eq!(after, HUD_PANEL, "панель ввода залита цветом HUD");
    assert_ne!(before, after, "активный ввод виден поверх HUD");
}

#[test]
fn typing_a_chat_message_logs_and_applies_it_in_demo_mode() {
    let (mut app, _tank, _enemy) = test_app();

    // Enter opens the input line.
    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Enter)),
        Transition::None
    );
    assert!(app.hud().chat.is_active());

    for character in "Helo".chars() {
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char(character)));
    }
    handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Backspace));
    for character in "lo".chars() {
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char(character)));
    }
    assert_eq!(app.hud().chat.message(), "Hello");

    // Enter sends: in demo mode — immediately via apply_command.
    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Enter)),
        Transition::None
    );
    assert!(!app.hud().chat.is_active());
    assert_eq!(
        app.state_ref().chat_log(),
        vec![(TeamId::RED, "Hello".to_string())]
    );
    assert!(
        app.hud()
            .log
            .lines()
            .iter()
            .any(|line| line == "RED: Hello"),
            "журнал содержит отправленное: {:?}",
        app.hud().log.lines()
    );
    assert_eq!(app.applied_commands(), 1);
    assert!(app.hud().pending.is_empty(), "демо-режим не копит очередь");
}

#[test]
fn lockstep_chat_is_queued_as_packet_18_for_the_session() {
    let (mut app, _tank, _enemy) = network_test_app();

    // T also opens the input line.
    handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char('T')));
    assert!(app.hud().chat.is_active());

    for character in "gg".chars() {
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char(character)));
    }
    handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Enter));

    assert!(
        app.state_ref().chat_log().is_empty(),
            "the lockstep session leaves the packet to net::flush_hud_orders"
    );

    let outgoing = app.take_outgoing_orders();

    assert_eq!(
        outgoing,
        vec![Command::Chat(Packet18Chat {
            sender: TeamId::RED,
            message: "gg".to_string(),
        })]
    );

    // The queue is exactly packet 18 on the wire.
    let bytes = Packet::Command(outgoing[0].clone())
        .encode()
        .expect("18 encoded");

    assert_eq!(bytes, [0x62, 0x00, 0x00, 0x03, 0x00, b'g', b'g', 0x00]);
}

#[test]
fn escape_cancels_chat_without_leaving_the_session() {
    let (mut app, _tank, _enemy) = test_app();

    handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char('t')));
    for character in "no".chars() {
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Char(character)));
    }

    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape)),
        Transition::None,
            "Esc в чате не выходит из сессии"
    );
    assert!(!app.hud().chat.is_active());
    assert!(app.state_ref().chat_log().is_empty());
    assert!(app
        .hud()
        .log
        .lines()
        .iter()
        .any(|line| line == "Chat cancelled"));

    // The next Esc — the usual session behavior already.
    assert_eq!(
        handle_session_input(&mut app, InputEvent::Key(crate::ui::Key::Escape)),
        Transition::LeaveSession
    );
}

/// [`air_move_over_foreign_unit`] — the `MOBILE_AIR_UNIT` branch of
/// `GameManager_GetWindowCursor` (`game_manager.cpp:4456-4464`): an aircraft in
/// focus moves over a foreign ground/sea unit (the plane hovers above it), while
/// an own unit (`CURSOR_FRIEND`), a unit the aircraft can attack (`CURSOR_ENEMY`)
/// and a fogged unit (`Access_GetEnemyUnit` needs `IsVisibleToTeam`) are not move
/// targets. An `ORDER_IDLE` aircraft sits in a container:
/// `GameManager_GetAirUnitCursor` (`:5937-5962`) handles that click instead.
#[test]
fn aircraft_moves_over_a_foreign_ground_unit_only() {
    let mut app = App::new(AppConfig::default().with_screen(512, 384));
    let universe = &mut app.universe;
    universe.set_player_team(TeamId::RED);
    *universe.team_units_mut(TeamId::RED).expect("RED economy") =
        TeamUnits::with_clan(TeamClan::Random);
    *universe.team_units_mut(TeamId::BLUE).expect("BLUE economy") =
        TeamUnits::with_clan(TeamClan::Random);

    let fighter = universe
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(10, 10),
            unit_flags::HOVERING,
        )
        .expect("fighter");
    let ground = universe
        .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(11, 10), 0)
        .expect("foreign tank");
    let own = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(12, 10), 0)
        .expect("own tank");
    let enemy_air = universe
        .spawn_unit(
            UnitType::Fighter,
            TeamId::BLUE,
            Point::new(13, 10),
            unit_flags::HOVERING,
        )
        .expect("foreign fighter");
    let fogged = universe
        .spawn_unit(UnitType::Tank, TeamId::BLUE, Point::new(14, 10), 0)
        .expect("fogged tank");
    let stored = universe
        .spawn_unit(
            UnitType::Fighter,
            TeamId::RED,
            Point::new(15, 10),
            unit_flags::HOVERING,
        )
        .expect("stored fighter");

    for id in [ground, enemy_air] {
        universe
            .unit_mut(id)
            .expect("unit")
            .set_visible_to_team(TeamId::RED, true);
    }
    {
        let unit = universe.unit_mut(stored).expect("stored fighter");
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::Store;
    }

    let state = app.state_ref();

    assert!(
        air_move_over_foreign_unit(state, fighter, ground),
        "an aircraft in focus moves over a foreign ground unit"
    );
    assert!(
        !air_move_over_foreign_unit(state, fighter, own),
        "an own unit is CURSOR_FRIEND: the click selects it"
    );
    assert!(
        !air_move_over_foreign_unit(state, fighter, enemy_air),
        "a valid attack target is CURSOR_ENEMY, not a move"
    );
    assert!(
        !air_move_over_foreign_unit(state, fighter, fogged),
        "a fogged unit is invisible to the C++ cursor"
    );
    assert!(
        !air_move_over_foreign_unit(state, fighter, stored),
        "an ORDER_IDLE aircraft goes through GameManager_GetAirUnitCursor"
    );
    assert!(
        !air_move_over_foreign_unit(state, own, ground),
        "a ground unit has no air move cursor"
    );
}
