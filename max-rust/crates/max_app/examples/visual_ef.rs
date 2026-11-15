//! Кадры-доказательства пунктов E/F: меню на оригинальных ресурсах и
//! целочисленный апскейл кадра (пункт F).
//!
//! Запуск из `max-rust/`:
//!
//! ```text
//! cargo run -p max_app --example visual_ef -- ../build/frame-ef
//! ```
//!
//! Пишет PPM (P6) 640×480:
//! * `…-main.ppm` — главное меню (`MAINPIC` + `BLANK_*`, наведение на
//!   `New Game`);
//! * `…-create.ppm` — форма создания на `PLANETSE` с сеткой 24 планет и
//!   `MNUBTN4/5/6`;
//! * `…-join.ppm` — список игр на `MULTGAME` в рамке с `BTNHIDE`;
//! * `…-main-2x.ppm` — тот же кадр главного меню после
//!   [`max_render::frame_scale::FrameScale`] 2× (1280×960, каждый пиксель
//!   блоком 2×2).

use max_app::menu;
use max_app::ui::{CreateForm, InputEvent, Screen, ScreenView};
use max_app::{write_ppm, App, AppConfig};
use max_net::GameInfo;
use max_render::buffer::RgbaBuffer;
use max_render::frame_scale::FrameScale;
use max_render::ui::Point;

fn game(id: &str, name: &str, players: u32, password: bool) -> GameInfo {
    GameInfo {
        id: id.to_string(),
        name: name.to_string(),
        world: 0,
        status: max_net::GameStatus::Running,
        port: 31554,
        players,
        min_players: 2,
        password_protected: password,
    }
}

fn main() {
    let prefix = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../build/frame-ef".to_string());
    // Данные игры — от корня репозитория (пример запускается из `max-rust/`).
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let config = AppConfig::default()
        .with_data_dir(root.join("max-original"))
        .with_assets_dir(root.join("assets"));
    let mut app = App::new(config);

    if let Err(error) = app.init_assets() {
        eprintln!("visual_ef: данные игры не загружены ({error:#}");
        return;
    }

    let menu_ready = app
        .assets()
        .and_then(|assets| assets.menu.as_ref())
        .is_some();
    println!("visual_ef: MenuAssets={menu_ready}");

    let view = |mouse: Option<(i32, i32)>, status: &'static str| ScreenView {
        width: 640,
        height: 480,
        status,
        mouse: mouse.map(|(x, y)| Point::new(x, y)),
        focus: 0,
        app: Some(&app),
    };

    let mut frames: Vec<(&str, RgbaBuffer)> = Vec::new();

    // Главное меню, курсор на «New Game»: hover не нажимает кнопку (*U).
    let mut main_frame = RgbaBuffer::new(640, 480);
    Screen::MainMenu.render(
        &mut main_frame,
        view(
            Some((385 + 100, 175 + 14)),
            "Main menu: New Game / Multiplayer Game / Exit",
        ),
    );
    frames.push(("main", main_frame.clone()));

    // Форма создания: выбранная планета 12 (New Luzon), курсор на Done.
    let form = CreateForm {
        name: "M.A.X. Rust".to_string(),
        world: 12,
        ..Default::default()
    };
    let mut create_frame = RgbaBuffer::new(640, 480);
    Screen::CreateForm(form).render(
        &mut create_frame,
        view(
            Some((514 + 54, 438 + 20)),
            "Click a planet; Tab/World - number; Enter - create; Esc - back",
        ),
    );
    frames.push(("create", create_frame));

    // Список игр: две игры, выбрана вторая.
    let games = vec![game("g1", "Alpha", 1, false), game("g2", "Beta", 2, true)];
    let mut join_frame = RgbaBuffer::new(640, 480);
    Screen::JoinList { games, selected: 1 }.render(
        &mut join_frame,
        view(
            Some((514 + 54, 438 + 20)),
            "2 game(s) — Enter joins, R refreshes",
        ),
    );
    frames.push(("join", join_frame));

    for (name, frame) in &frames {
        let path = format!("{prefix}-{name}.ppm");
        if let Err(error) = write_ppm(&path, frame) {
            eprintln!("visual_ef: {path}: {error}");
        } else {
            println!("ppm: {path} ({}x{})", frame.width(), frame.height());
        }
    }

    // Пункт F: 640×480 → 1280×960, каждый пиксель блоком 2×2.
    let scale = FrameScale::native(1280, 960);
    let upscaled = scale.upscale(&frames[0].1);
    let path = format!("{prefix}-main-2x.ppm");

    if let Err(error) = write_ppm(&path, &upscaled) {
        eprintln!("visual_ef: {path}: {error}");
    } else {
        println!(
            "ppm: {path} ({}x{}, scale {}, offset {:?})",
            upscaled.width(),
            upscaled.height(),
            scale.scale(),
            scale.offset()
        );
    }

    // Клик в окне 1280×960 → клетка карты в кадре (пункт F).
    let frame_click = scale.frame_to_window(menu::MENU_WIDTH / 2, menu::MENU_HEIGHT / 2);
    println!(
        "click window {:?} → frame {:?}",
        (frame_click.0 + 1, frame_click.1 + 1),
        scale.window_to_frame(frame_click.0 + 1, frame_click.1 + 1)
    );

    // Проверка, что клик действительно проходит через ту же машину экранов.
    let client = max_app::ui::HorizonMenuClient::new("127.0.0.1", 8082);
    let mut ctx = max_app::ui::UiContext::new(Box::new(client), 640, 480);
    ctx.status = "visual_ef".to_string();
    let mut screen = Screen::MainMenu;
    let _ = screen.handle_input(&mut ctx, InputEvent::MouseMove(400, 190));
    println!("screens: {screen:?}");
}
