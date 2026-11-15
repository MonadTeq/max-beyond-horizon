//! Кадры-доказательства M-SP: настройка (три экрана), партия с ИИ, победа.
//!
//! ```sh
//! cargo run -p max_app --example sp_frames -- ../max-original ../build
//! ```
//!
//! Пишет PPM (P6) в каталог вывода:
//!
//! * `sp-options.ppm` — `GameConfigMenu` (фон `OPTNFRM`);
//! * `sp-planets.ppm` — `PlanetSelectMenu` (фон `PLANETSE`, 3×2 мини-карты);
//! * `sp-players.ppm` — `ChoosePlayerMenu` (фон `CHOSPLYR`, 4 команды);
//! * `sp-game.ppm` — партия после хода ИИ (карта + хром 1:1);
//! * `sp-victory.ppm` — экран победы (`ENDGAME*` + статистика);
//! * `sp-defeat.ppm` — экран поражения.

use std::path::PathBuf;

use max_app::local::{LocalPhase, LocalSetup};
use max_app::setup::{GameSetupForm, SetupScreen};
use max_app::ui::{Screen, UiContext};
use max_app::{write_ppm, App, AppConfig, AppMode};
use max_core::enums::{PlayMode, VictoryType};
use max_core::team::{TeamId, TeamKind};
use max_render::buffer::RgbaBuffer;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let data_dir = PathBuf::from(args.next().unwrap_or_else(|| "../max-original".to_string()));
    let out_dir = PathBuf::from(args.next().unwrap_or_else(|| "../build".to_string()));

    std::fs::create_dir_all(&out_dir)?;

    let config = AppConfig::default()
        .with_data_dir(&data_dir)
        .with_assets_dir("../assets")
        .with_mode(AppMode::Ui);

    let mut app = App::new(config);
    app.init_assets()?;

    let mut ctx = UiContext::new(Box::new(NullClient), 640, 480);
    ctx.player_name = "Commander".to_string();
    ctx.world_dir = data_dir.clone();

    let palette = app
        .assets()
        .and_then(|assets| assets.menu.as_ref())
        .map(|menu| menu.palette);

    let mut buffer = RgbaBuffer::new(640, 480);

    // --- 1. Options --------------------------------------------------------
    let mut form = GameSetupForm::default();
    form.setup.player_name = "Commander".to_string();
    form.world_dir = data_dir.clone();
    form.status = "Options: difficulty, timers, credits, resources, victory".to_string();

    let screen = Screen::LocalSetup(form.clone());
    screen.render(&mut buffer, ctx.view(Some(&app)));
    let path = out_dir.join("sp-options.ppm");
    write_ppm(&path, &buffer)?;
    println!("{}: экран опций", path.display());

    // --- 2. Planet ---------------------------------------------------------
    form.screen = SetupScreen::Planet;
    form.setup.world = 4;
    form.ensure_minimaps(palette.as_ref());

    let minimaps = form.minimaps.iter().filter(|entry| entry.is_some()).count();
    println!("мини-карты планет: {minimaps}/24");

    let screen = Screen::LocalSetup(form.clone());
    screen.render(&mut buffer, ctx.view(Some(&app)));
    let path = out_dir.join("sp-planets.ppm");
    write_ppm(&path, &buffer)?;
    println!("{}: выбор планеты", path.display());

    // --- 3. Players --------------------------------------------------------
    form.screen = SetupScreen::Players;
    form.setup.team_types = [
        TeamKind::Player,
        TeamKind::Computer,
        TeamKind::Computer,
        TeamKind::Computer,
    ];
    form.status = "Done starts the game".to_string();

    let screen = Screen::LocalSetup(form.clone());
    screen.render(&mut buffer, ctx.view(Some(&app)));
    let path = out_dir.join("sp-players.ppm");
    write_ppm(&path, &buffer)?;
    println!("{}: выбор команд", path.display());

    // --- 4. Партия с ИИ ----------------------------------------------------
    let setup = LocalSetup {
        player_name: "Commander".to_string(),
        world: 4,
        opponent: 2,
        timer: 0,
        endturn: 0,
        play_mode: PlayMode::TurnBased,
        start_gold: 150,
        raw: 1,
        fuel: 1,
        gold: 1,
        derelicts: 0,
        victory_type: VictoryType::Duration,
        victory_limit: 50,
        team_types: [
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::Computer,
            TeamKind::None,
        ],
        seed: 11,
    };

    app.start_local_game(&setup)?;

    // Ход игрока → ход ИИ → возврат к игроку (2 раунда).
    for _ in 0..2 {
        app.local_end_turn();
        for _ in 0..40_000 {
            if app.local().map(|local| local.phase) == Some(LocalPhase::PlayerTurn) {
                break;
            }
            if app.local().is_none_or(|local| local.finished()) {
                break;
            }
            app.local_update(0.0);
        }
    }

    // Немного тиков хода игрока, чтобы юниты встали на места.
    for _ in 0..6 {
        app.local_update(0.0);
    }

    if let Some(local) = app.local() {
        ctx.status = local.status_line(app.state().turn_counter);
    }

    let screen = Screen::InSession {
        request: max_app::SessionRequest {
            game_id: "local".to_string(),
            password: None,
        },
    };
    screen.render(&mut buffer, ctx.view(Some(&app)));
    let path = out_dir.join("sp-game.ppm");
    write_ppm(&path, &buffer)?;
    println!(
        "{}: партия с ИИ (ход {}, юнитов {})",
        path.display(),
        app.state().turn_counter,
        app.state().unit_count()
    );

    // --- 5. Победа ---------------------------------------------------------
    let ai_units: Vec<_> = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team != app.state().player_team)
        })
        .collect();
    for id in ai_units {
        app.state_mut().destroy_unit(id);
    }

    app.local_end_turn();
    for _ in 0..60_000 {
        if app.local().is_some_and(|local| local.finished()) {
            break;
        }
        if app.local().is_none() {
            break;
        }
        app.local_update(0.0);
    }

    let screen = Screen::GameOver;
    screen.render(&mut buffer, ctx.view(Some(&app)));
    let path = out_dir.join("sp-victory.ppm");
    write_ppm(&path, &buffer)?;
    println!(
        "{}: экран победы (исход: {:?})",
        path.display(),
        app.local()
            .and_then(|local| local.outcome.as_ref())
            .map(|outcome| outcome.victory)
    );

    // --- 6. Поражение ------------------------------------------------------
    // Новая партия, гибнут юниты игрока: статус «Lost» → поражение.
    app.start_local_game(&setup)?;
    let player = app.state().player_team;
    let own_units: Vec<_> = app
        .state()
        .unit_ids_in_order()
        .into_iter()
        .filter(|id| {
            app.state()
                .unit(*id)
                .is_some_and(|unit| unit.team == player)
        })
        .collect();
    for id in own_units {
        app.state_mut().destroy_unit(id);
    }

    app.local_end_turn();
    for _ in 0..60_000 {
        if app.local().is_some_and(|local| local.finished()) {
            break;
        }
        if app.local().is_none() {
            break;
        }
        app.local_update(0.0);
    }

    let screen = Screen::GameOver;
    screen.render(&mut buffer, ctx.view(Some(&app)));
    let path = out_dir.join("sp-defeat.ppm");
    write_ppm(&path, &buffer)?;
    println!(
        "{}: экран поражения (исход: {:?})",
        path.display(),
        app.local()
            .and_then(|local| local.outcome.as_ref())
            .map(|outcome| outcome.victory)
    );

    // Проверка команд партии (не печатаем «Alien»).
    let _ = TeamId::RED;
    Ok(())
}

/// Заглушка клиента Horizon: кадрам M-SP сеть не нужна.
struct NullClient;

impl max_app::ui::MenuClient for NullClient {
    fn list_games(&mut self) -> Result<Vec<max_net::GameInfo>, String> {
        Err("offline".to_string())
    }

    fn create_game(
        &mut self,
        _params: max_net::CreateGameParams,
    ) -> Result<max_net::GameInfo, String> {
        Err("offline".to_string())
    }
}
