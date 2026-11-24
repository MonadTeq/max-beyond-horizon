//! Blinking green «New … available» label over a unit that waits for the player.
//!
//! Oracle: `DrawMap_RenderNamesDisplay` (`drawmap.cpp:747-777`).
//!
//! * a factory in `ORDER_BUILD`/`ORDER_STATE_UNIT_READY` that holds a produced unit
//!   (`UnitInfo::SpawnNewUnit`, `unitinfo.cpp:3489-3520`) draws
//!   `DrawMap_UnitCompletionLabels` (`_(d7d0)/(2ec0)/(7c23)`, en-US
//!   `New %s available`) over itself — `%s` is the **parent's** (the stored unit's)
//!   singular name;
//! * a mobile builder in the same state draws the same text over the finished
//!   building (`if (parent->flags & STATIONARY) unit = parent`);
//! * a powered-on `RESEARCH` with a finished topic draws `_(e3a4)`
//!   (`%s research completed`);
//! * both use `GNW_TEXT_OUTLINE | 0x1F`: palette slot 31, whose green channel is the
//!   6 fps single-slot palette cycle (`World::InitColorCycles`,
//!   `world.cpp:551` / `:222-231`) — that is the original's blinking green.
//!
//! The label is not part of the NAME toggle (`GameManager_DisplayButtonNames`):
//! C++ calls `DrawMap_RenderNamesDisplay` for every own unit of the draw group.
//!
//! Tests are skipped if there is no game data (`max-original/`) nearby.

use std::path::{Path, PathBuf};

use max_app::overlays::{ready_label_color, unit_ready_label};
use max_app::{App, AppConfig};
use max_core::enums::{ResearchTopic, UnitOrder, UnitOrderState, UnitType};
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::unit::UnitId;
use max_render::buffer::RgbaBuffer;

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative)
}

fn app_with_assets() -> Option<App> {
    let max_res = repo_path("max-original/MAX.RES");
    let color_pal = repo_path("max-original/COLOR.PAL");
    let patches = repo_path("assets/PATCHES.RES");

    if !max_res.is_file() || !color_pal.is_file() || !patches.is_file() {
        eprintln!("unit_ready_label: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");
    app.universe_mut().teams_mut()[0].team_type = TeamKind::Player;
    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(40, 40);

    Some(app)
}

fn count_color(frame: &RgbaBuffer, color: (u8, u8, u8)) -> usize {
    frame
        .pixels()
        .chunks_exact(4)
        .filter(|pixel| (pixel[0], pixel[1], pixel[2]) == color)
        .count()
}

/// A factory that produced a unit: the label is on the factory, its text names the
/// produced unit, and it is drawn even with every panel button off.
#[test]
fn factory_with_a_produced_unit_shows_the_label() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let factory = app
        .universe_mut()
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(40, 40), 0)
        .expect("factory");
    let infantry = app
        .universe_mut()
        .spawn_unit(UnitType::Infantry, TeamId::RED, Point::new(44, 44), 0)
        .expect("infantry");

    {
        // `UnitInfo::SpawnNewUnit`, stationary branch: the factory is in
        // `ORDER_BUILD`/`UNIT_READY`, holds the unit (`storage = 1`) and both point
        // at each other.
        let unit = app.universe_mut().unit_mut(factory).expect("factory");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        unit.parent_unit = Some(infantry);
        unit.storage = 1;
    }
    {
        let unit = app.universe_mut().unit_mut(infantry).expect("infantry");
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::Store;
        unit.parent_unit = Some(factory);
    }

    // The NAME/STATUS/... buttons are off by default; the label must still appear.
    assert!(!app.hud().panels.names);

    {
        let state = app.state_ref();
        let label =
            unit_ready_label(state, state.unit(factory).expect("factory")).expect("надпись");
        assert_eq!(label.0, "New Infantry available");
        assert_eq!(label.1, factory, "надпись на самом заводе");
    }

    let with_label = app.render_frame();
    let green = ready_label_color(&app);
    assert_eq!(
        (green.r, green.g, green.b),
        (0, 252, 0),
        "dt = 0: стартовая фаза цикла слота 31"
    );

    // Baseline: the same factory without the `UNIT_READY` state.
    app.universe_mut().unit_mut(factory).expect("factory").state = UnitOrderState::ExecutingOrder;
    let without_label = app.render_frame();

    let painted = count_color(&with_label, (green.r, green.g, green.b));
    let plain = count_color(&without_label, (green.r, green.g, green.b));
    assert!(
        painted > plain,
        "зелёная надпись не нарисована: {painted} против {plain} пикселей"
    );
    assert!(
        unit_ready_label(
            app.state_ref(),
            app.state_ref().unit(factory).expect("factory")
        )
        .is_none(),
        "без UNIT_READY надписи нет"
    );
}

/// A builder that finished a building: the same text, but over the building.
#[test]
fn finished_building_gets_the_label_from_its_builder() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let building = app
        .universe_mut()
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(40, 40), 0)
        .expect("building");
    let builder = app
        .universe_mut()
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(42, 42), 0)
        .expect("builder");

    {
        let unit = app.universe_mut().unit_mut(builder).expect("builder");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        unit.parent_unit = Some(building);
    }
    {
        let unit = app.universe_mut().unit_mut(building).expect("building");
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::BuildingReady;
        unit.parent_unit = Some(builder);
    }

    let state = app.state_ref();
    let label = unit_ready_label(state, state.unit(builder).expect("builder")).expect("надпись");
    assert_eq!(label.0, "New Training Hall available");
    assert_eq!(
        label.1, building,
        "STATIONARY parent: надпись переезжает на здание"
    );
}

/// Another team's ready unit gets no label (C++: `unit->team == GameManager_PlayerTeam`).
#[test]
fn foreign_unit_has_no_label() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let factory = app
        .universe_mut()
        .spawn_unit(UnitType::TrainingHall, TeamId::GREEN, Point::new(40, 40), 0)
        .expect("factory");
    let infantry = app
        .universe_mut()
        .spawn_unit(UnitType::Infantry, TeamId::GREEN, Point::new(44, 44), 0)
        .expect("infantry");

    {
        let unit = app.universe_mut().unit_mut(factory).expect("factory");
        unit.orders = UnitOrder::Build;
        unit.state = UnitOrderState::UnitReady;
        unit.parent_unit = Some(infantry);
    }

    let frame = app.render_frame();
    let green = ready_label_color(&app);
    assert_eq!(
        count_color(&frame, (green.r, green.g, green.b)),
        0,
        "надпись только для своей команды"
    );
}

/// `%s research completed` over a powered-on research center with a finished topic.
#[test]
fn finished_research_gets_the_label() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let center = app
        .universe_mut()
        .spawn_unit(UnitType::ResearchCentre, TeamId::RED, Point::new(40, 40), 0)
        .expect("research");

    {
        let unit = app.universe_mut().unit_mut(center).expect("research");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.research_topic = ResearchTopic::Attack;
    }
    app.universe_mut().teams_mut()[0].research_topics[ResearchTopic::Attack as usize]
        .turns_to_complete = 0;

    let state = app.state_ref();
    let label = unit_ready_label(state, state.unit(center).expect("research")).expect("надпись");
    assert_eq!(label.0, "Attack research completed");
    assert_eq!(label.1, center);

    let frame = app.render_frame();
    let green = ready_label_color(&app);
    assert!(
        count_color(&frame, (green.r, green.g, green.b)) > 0,
        "надпись о завершённом исследовании не нарисована"
    );
}

/// The label's palette slot blinks: the green channel is the 6 fps single-slot
/// cycle `63 → 31 → 15 → 7` (`World::UpdateColorAnimations`).
#[test]
fn label_color_blinks_with_the_palette_cycle() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let start = ready_label_color(&app);
    assert_eq!((start.r, start.g, start.b), (0, 252, 0));

    // 200 ms — one step of the 6 fps slot (166 ms): 63 / 2 = 31 (6-bit).
    app.update_visual_units(0.2);
    let first = ready_label_color(&app);
    assert_eq!(
        (first.r, first.g, first.b),
        (0, 124, 0),
        "второй кадр мигания"
    );

    // +340 ms — two more steps: 15 and 7 (6-bit) → 60…28.
    app.update_visual_units(0.34);
    let third = ready_label_color(&app);
    assert!(
        third.g <= 60 && third.g >= 28,
        "третий-четвёртый кадр мигания: зелёный канал {}",
        third.g
    );

    // `dt = 0` (QA/headless) resets the cycle: the deterministic bright phase again.
    app.update_visual_units(0.0);
    assert_eq!(ready_label_color(&app).g, 252);
}

/// The label is drawn for the factory that keeps a ready unit, not for the unit
/// inside it (`UnitInfoGroup::IsRelevant` skips stored units).
#[test]
fn stored_unit_itself_has_no_label() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    let factory = app
        .universe_mut()
        .spawn_unit(UnitType::TrainingHall, TeamId::RED, Point::new(40, 40), 0)
        .expect("factory");
    let infantry: UnitId = app
        .universe_mut()
        .spawn_unit(UnitType::Infantry, TeamId::RED, Point::new(44, 44), 0)
        .expect("infantry");

    {
        let unit = app.universe_mut().unit_mut(infantry).expect("infantry");
        unit.orders = UnitOrder::Idle;
        unit.state = UnitOrderState::Store;
        unit.parent_unit = Some(factory);
    }

    let state = app.state_ref();
    assert!(
        unit_ready_label(state, state.unit(infantry).expect("infantry")).is_none(),
        "у сохранённого юнита собственной надписи нет"
    );
}
