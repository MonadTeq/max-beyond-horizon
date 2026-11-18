//! Стройплощадка на карте: реальные декали C++ (`LRGTAPE`/`LRGCONES`/
//! `LRGSLAB`) видны в кадре сразу после заказа.
//!
//! Оракул — `unitinfo.cpp:3802` (`DeployConstructionSiteUtilities`) и
//! `unitinfo.cpp:4122` (`PrepareConstructionSite`): площадка — это не
//! «процент прогресса», а три настоящих юнита-декали в клетке постройки.
//! `App::draw_units` рисует их общим циклом спрайтов (они `ORDER_AWAIT`, не
//! `ORDER_IDLE`), поэтому кадр обязан измениться ещё до первого хода.
//!
//! Тест пропускается, если данных игры (`max-original/`) рядом нет.

use std::path::{Path, PathBuf};

use max_app::{App, AppConfig};
use max_core::enums::UnitType;
use max_core::geo::Point;
use max_core::production::{find_build_site, ProductionManager};
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
        // 640×640 — полноэкранный fallback без `FRAMEPIC`; карта целиком наша.
        .with_screen(640, 640);
    let mut app = App::new(config);
    app.init_assets().expect("ассеты игры");

    Some(app)
}

/// Кадр «до заказа» против кадра «после заказа»: площадка обязана появиться на
/// карте (а не только в состоянии), а после достройки — исчезнуть вместе с
/// лентой/конусами, оставив плиту и готовое здание.
#[test]
fn construction_site_is_drawn_on_the_map_until_the_building_is_ready() {
    let Some(mut app) = app_with_assets() else {
        return;
    };

    app.state_mut().teams[0].team_type = TeamKind::Player;
    app.state_mut().team_units[0] = max_core::TeamUnits::with_clan(TeamClan::Random);
    app.state_mut().team_units[0].set_gold(100);

    let constructor = app
        .state_mut()
        .spawn_unit(UnitType::Constructor, TeamId::RED, Point::new(40, 40), 0)
        .expect("constructor");

    let site = find_build_site(app.state(), TeamId::RED, UnitType::TrainingHall, Point::new(40, 40))
        .expect("site");

    app.viewport_mut().set_zoom(64);
    app.viewport_mut().center_on_grid(site.x, site.y);

    let (before, before_stats) = app.render_frame_with_stats();

    let mut manager = ProductionManager::new(app.state_mut(), TeamId::RED);
    manager
        .enqueue(constructor, UnitType::TrainingHall)
        .expect("queue");
    let report = manager.commit(constructor).expect("build order");
    drop(manager);

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

    // Достройка: лента и конусы убираются, плита и здание остаются.
    for _ in 0..report.turns {
        max_core::production::progress_build_state(app.state_mut(), TeamId::RED);
    }

    let (after, after_stats) = app.render_frame_with_stats();
    let tapes = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter_map(|id| app.state().unit(id))
        .filter(|unit| {
            matches!(
                unit.unit_type,
                UnitType::LargeTape | UnitType::SmallTape | UnitType::LargeCones | UnitType::SmallCones
            )
        })
        .count();
    assert_eq!(tapes, 0, "лента/конусы снимаются после стройки");
    assert_eq!(
        after_stats.units_drawn,
        site_stats.units_drawn - 1,
        "минус лента и конусы, плюс готовое здание"
    );
    assert!(
        after.pixels() != with_site.pixels(),
        "кадр после достройки обязан отличаться от площадки"
    );
}
