//! Тонкий SDL3-слой: окно, текстура кадра, события.
//!
//! Модуль собирается только под feature `sdl` (см. `Cargo.toml`), поэтому
//! чистая часть крейта и все её тесты работают без установленного SDL3.
//! Здесь нет игровой логики и нет отрисовки как таковой: слой принимает готовый
//! [`RgbaBuffer`], заливает его в потоковую текстуру и отдаёт окну.
//!
//! Почему SDL3, а не SDL2: C++-порт (`max-cpp-port`) и оригинальная игра
//! используют SDL3, поэтому окно и ввод совпадают по API с эталоном. Крейт
//! `sdl3` (crates.io) требует либо системного SDL3 (`libsdl3-dev`, vcpkg), либо
//! сборки из исходников:
//!
//! ```text
//! # вариант 1 — системный SDL3 (Linux: apt install libsdl3-dev; Windows: vcpkg):
//! cargo run -p max_render --features sdl --example window
//!
//! # вариант 2 — SDL3 собирается из исходников (нужен cmake; docker/CI):
//! cargo run -p max_render --features sdl-from-source --example window
//! ```
//!
//! Запуск без feature `sdl` даёт ошибку компиляции у вызывающего кода
//! (модуля просто нет) — это осознанно: SDL не должен протекать в чистые тесты.

use sdl3::pixels::PixelFormat;
use sdl3::render::{Canvas, TextureCreator};
use sdl3::video::{Window, WindowContext};
use sdl3::EventPump;

use crate::buffer::RgbaBuffer;

/// Ошибка инициализации окна/вывода кадра.
#[derive(Debug)]
pub struct WindowError {
    /// Этап, на котором произошла ошибка (`init`, `video`, `window`, …).
    pub stage: &'static str,
    /// Сообщение SDL.
    pub message: String,
}

impl WindowError {
    fn new(stage: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            stage,
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SDL3 ({}): {}", self.stage, self.message)
    }
}

impl std::error::Error for WindowError {}

/// Окно SDL3 с потоковой текстурой под RGBA8-кадр.
///
/// Текстура создаётся на каждый [`SdlWindow::present`] — крейт `sdl3` даёт
/// `Texture<'creator>`, которую нельзя хранить рядом с её `TextureCreator` в
/// одной структуре без `unsafe` (а `unsafe` в этом крейте запрещён). Для
/// клиента это неважно: SDL переиспользует драйверный буфер, а кадр всё равно
/// перерисовывается целиком.
pub struct SdlWindow {
    /// Контекст SDL держим живым на всё время жизни окна.
    _sdl: sdl3::Sdl,
    canvas: Canvas<Window>,
    creator: TextureCreator<WindowContext>,
    events: EventPump,
    title: String,
}

impl SdlWindow {
    /// Создаёт окно `width × height` с заголовком `title`.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, WindowError> {
        let sdl = sdl3::init().map_err(|error| WindowError::new("init", error))?;
        let video = sdl
            .video()
            .map_err(|error| WindowError::new("video", error))?;
        let window = video
            .window(title, width, height)
            .position_centered()
            .build()
            .map_err(|error| WindowError::new("window", error))?;
        let canvas: Canvas<Window> = window.into_canvas();
        let creator = canvas.texture_creator();
        let events = sdl
            .event_pump()
            .map_err(|error| WindowError::new("event_pump", error))?;

        Ok(Self {
            _sdl: sdl,
            canvas,
            creator,
            events,
            title: title.to_owned(),
        })
    }

    /// Размер окна в пикселях.
    pub fn size(&self) -> (u32, u32) {
        self.canvas.window().size()
    }

    /// Заголовок окна.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Меняет заголовок окна.
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_owned();
        self.canvas.window_mut().set_title(title).ok();
    }

    /// Выводит кадр: заливает текстуру и растягивает её на всё окно.
    ///
    /// Буфер должен иметь непрозрачную альфу там, где ожидается картинка
    /// (SDL смешивает RGBA-текстуру с чёрным фоном окна).
    pub fn present(&mut self, buffer: &RgbaBuffer) -> Result<(), WindowError> {
        if buffer.is_empty() {
            return Ok(());
        }

        let mut texture = self
            .creator
            .create_texture_streaming(PixelFormat::RGBA32, buffer.width(), buffer.height())
            .map_err(|error| WindowError::new("texture", error))?;

        texture
            .update(None, buffer.pixels(), buffer.stride())
            .map_err(|error| WindowError::new("texture_update", error))?;

        self.canvas
            .copy(&texture, None, None)
            .map_err(|error| WindowError::new("copy", error))?;
        let _ = self.canvas.present();

        Ok(())
    }

    /// Заливает окно цветом (например, перед первым кадром).
    pub fn clear(&mut self, color: crate::buffer::Rgba) {
        self.canvas.set_draw_color(sdl3::pixels::Color::RGBA(
            color.r, color.g, color.b, color.a,
        ));
        self.canvas.clear();
    }

    /// Опрашивает очередь событий.
    ///
    /// Возвращает `false`, когда окно пора закрывать (закрытие окна или Esc).
    pub fn poll(&mut self) -> bool {
        let mut running = true;

        for event in self.events.poll_iter() {
            match event {
                sdl3::event::Event::Quit { .. } => running = false,
                sdl3::event::Event::KeyDown {
                    keycode: Some(sdl3::keyboard::Keycode::Escape),
                    ..
                } => running = false,
                _ => {}
            }
        }

        running
    }
}
