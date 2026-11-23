//! The construction site on the map: the real C++ decals (`LRGTAPE`/`LRGCONES`/
//! `LRGSLAB`) are visible in the frame right after the order.
//!
//! Oracle — `unitinfo.cpp:3802` (`DeployConstructionSiteUtilities`) and
//! `unitinfo.cpp:4122` (`PrepareConstructionSite`): the site is not a
//! "progress percentage" but three real decal units in the construction cell.
//! `App::draw_units` draws them in the common sprite loop (they are `ORDER_AWAIT`, not
//! `ORDER_IDLE`), so the frame must change before the first turn.
//!
//! The test is skipped if there is no game data (`max-original/`) nearby.

use std::path::{Path, PathBuf};

use max_app::{App, AppConfig};
use max_core::enums::UnitType;
use max_core::geo::Point;
use max_core::team::{TeamId, TeamKind};
use max_core::TeamClan;

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
        eprintln!("construction_site: пропуск — нет данных игры рядом с репозиторием");
        return None;
    }

    let config = AppConfig::default()
        .with_data_dir(repo_path("max-original"))
        .with_assets_dir(repo_path("assets"))
        // 640×640 — the full-screen fallback without `FRAMEPIC`; the map is entirely ours.
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// How many pixels differ between two frames (RGBA, exact comparison).
fn changed_pixels(
    left: &max_render::buffer::RgbaBuffer,
    right: &max_render::buffer::RgbaBuffer,
) -> usize {
    left.pixels()
        .chunks_exact(4)
        .zip(right.pixels().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}

/// The "before the order" frame vs the "after the order" frame: the site must appear on
/// the map (and not only in the state), and after completion — disappear together with
/// the tape/cones, leaving the slab and the finished building.
#[test]
fn construction_site_is_drawn_on_the_map_until_the_building_is_ready() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    app.universe.teams_mut()[0].team_type = TeamKind::Player;
    app.universe.team_units_table_mut()[0] = max_core::TeamUnits::with_clan(TeamClan::Random);
    app.universe.team_units_table_mut()[0].set_gold(100);

    let constructor = app
        .universe
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(40, 40), 0)
        .expect("constructor");
    app.universe
        .unit_mut(constructor)
        .expect("constructor")
        .storage = 40;

    let site = app.state_ref().find_build_site( TeamId::RED, UnitType::TrainingHall, Point::new(40, 40))
        .expect("site");

    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(site.x, site.y);

    let (before, before_stats) = app.render_frame_with_stats();

    app.universe
        .enqueue_build(TeamId::RED, constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = app
        .universe
        .commit_build(TeamId::RED, constructor)
        .expect("build order");

    assert_eq!(report.site, site, "детерминированная площадка");

    let (with_site, site_stats) = app.render_frame_with_stats();
    assert_eq!(
        site_stats.units_drawn,
        before_stats.units_drawn + 3,
        "на карте обязаны нарисоваться лента, конусы и плита"
    );

    let changed = before
        .pixels()
        .chunks_exact(4)
        .zip(with_site.pixels().chunks_exact(4))
        .filter(|(left, right)| left != right)
        .count();
    assert!(
        changed > 500,
        "площадка должна быть видна в кадре: изменено {changed} пикселей"
    );

    // Each decal must give **its own** pixels: if the tape, cones or
    // slab are not drawn (e.g. the decal remained `ORDER_IDLE` or is absent
    // in the frame), the frame without it will not change. We check them in turn, like `IsRelevant`
    // for each unit (`unitinfogroup.cpp:59`).
    let site_units = app.state_ref().units_at(site);
    let find_site_unit = |unit_type: UnitType| {
        site_units.iter().copied().find(|id| {
            app.state_ref()
                .unit(*id)
                .is_some_and(|unit| unit.unit_type == unit_type)
        })
    };

    for (label, decal) in [
        ("лента LRGTAPE", find_site_unit(UnitType::LargeTape)),
        ("конусы LRGCONES", find_site_unit(UnitType::LargeCones)),
        ("плита LRGSLAB", find_site_unit(UnitType::LargeSlab)),
    ] {
        let Some(decal) = decal else {
            panic!("{label} нет в клетке площадки");
        };

        let order = app
            .state_ref()
            .unit(decal)
            .map(|unit| (unit.orders, unit.state))
            .expect("декаль на площадке");

        app.universe
            .unit_mut(decal)
            .expect("декаль на площадке")
            .set_visible_to_team(TeamId::RED, false);
        let without = app.render_frame();
        app.universe
            .unit_mut(decal)
            .expect("декаль на площадке")
            .set_visible_to_team(TeamId::RED, true);

        let own = changed_pixels(&with_site, &without);

        assert!(
            own > 100,
            "{label} не даёт пикселей в кадре (своих: {own}; orders={:?}/{:?})",
            order.0,
            order.1
        );
    }

    // Completion (`UnitInfo::SpawnNewUnit`): the building stands on the site in
    // `ORDER_IDLE`/`ORDER_STATE_BUILDING_READY`, so it is **not visible**
    // (`IsRelevant`), while the tape/cones and the builder are in place.
    for _ in 0..report.turns {
        app.universe.progress_build_state( TeamId::RED);
    }

    let (waiting, waiting_stats) = app.render_frame_with_stats();
    let tapes = |app: &App| {
        app.state_ref()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state_ref().unit(id))
            .filter(|unit| {
                matches!(
                    unit.unit_type,
                    UnitType::LargeTape
                        | UnitType::SmallTape
                        | UnitType::LargeCones
                        | UnitType::SmallCones
                )
            })
            .count()
    };

    assert_eq!(tapes(&app), 2, "лента и конусы ещё на площадке");
    assert_eq!(
        waiting_stats.units_drawn, site_stats.units_drawn,
        "ждущее здание не рисуется (`ORDER_IDLE`), кадр не меняется по составу"
    );
    assert_eq!(
        waiting.pixels(),
        with_site.pixels(),
        "кадр не должен меняться, пока строитель не ушёл с площадки"
    );

    // The builder leaves + `UnitsManager_BuildingReady`: the building opens, and
    // only now the tape/cones are removed, while the slab remains as the foundation.
    for _ in 0..40 {
        app.tick();
    }

    let (after, after_stats) = app.render_frame_with_stats();
    assert_eq!(tapes(&app), 0, "лента/конусы снимаются после ухода строителя");
    assert_eq!(
        after_stats.units_drawn,
        site_stats.units_drawn - 1,
        "минус лента и конусы, плюс готовое здание"
    );
    assert!(
        after.pixels() != with_site.pixels(),
        "кадр после достройки обязан отличаться от площадки"
    );
    assert!(
        app.state_ref()
            .unit_ids_in_order()
            .into_iter()
            .filter_map(|id| app.state_ref().unit(id))
            .any(|unit| unit.unit_type == UnitType::LargeSlab),
        "плита остаётся под зданием (`remove_slabs = false`)"
    );
}
