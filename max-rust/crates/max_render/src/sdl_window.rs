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

use std::collections::HashMap;

use sdl3::keyboard::Keycode;
use sdl3::mouse::{Cursor, MouseButton};
use sdl3::pixels::PixelFormat;
use sdl3::rect::Rect as SdlRect;
use sdl3::render::{Canvas, ScaleMode, TextureCreator};
use sdl3::surface::Surface;
use sdl3::video::{Window, WindowContext};
use sdl3::EventPump;

use crate::buffer::RgbaBuffer;
use crate::frame_scale::FrameScale;

/// Клавиша окна в терминах UI: `max_app` не зависит от типов SDL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKey {
    /// Esc.
    Escape,
    /// Enter / KpEnter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Стрелка вверх.
    Up,
    /// Стрелка вниз.
    Down,
    /// Стрелка влево.
    Left,
    /// Стрелка вправо.
    Right,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Печатный ASCII-символ (SDL отдаёт базовую раскладку: `a`, `0`, `.`).
    Char(char),
    /// Любая другая клавиша (модификаторы, F-клавиши, …).
    Other,
}

/// Событие окна, пригодное для UI-цикла без протекания SDL-типов.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowInput {
    /// Нажатие клавиши (автоповтор отфильтрован).
    Key(WindowKey),
    /// Движение мыши в пикселях окна.
    MouseMove {
        /// Координата x.
        x: i32,
        /// Координата y.
        y: i32,
    },
    /// Нажатие левой кнопки мыши.
    MouseClick {
        /// Координата x.
        x: i32,
        /// Координата y.
        y: i32,
    },
    /// Нажатие правой кнопки мыши (в игре — приказ движения/отмена).
    MouseRightClick {
        /// Координата x.
        x: i32,
        /// Координата y.
        y: i32,
    },
}

/// Переводит SDL-клавишу в [`WindowKey`].
fn window_key(code: Keycode) -> WindowKey {
    match code {
        Keycode::Escape => WindowKey::Escape,
        Keycode::Return | Keycode::Return2 | Keycode::KpEnter => WindowKey::Enter,
        Keycode::Tab | Keycode::KpTab => WindowKey::Tab,
        Keycode::Backspace | Keycode::KpBackspace => WindowKey::Backspace,
        Keycode::Up => WindowKey::Up,
        Keycode::Down => WindowKey::Down,
        Keycode::Left => WindowKey::Left,
        Keycode::Right => WindowKey::Right,
        Keycode::PageUp => WindowKey::PageUp,
        Keycode::PageDown => WindowKey::PageDown,
        _ => {
            // Базовые ASCII-коды SDL3 совпадают с ASCII (`A = 97`, `_0 = 48`).
            match code as u32 {
                raw @ 0x20..=0x7E => WindowKey::Char(char::from_u32(raw).unwrap_or('?')),
                _ => WindowKey::Other,
            }
        }
    }
}

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
    canvas: Canvas<Window>,
    creator: TextureCreator<WindowContext>,
    events: EventPump,
    title: String,
    /// Логический размер кадра (640×480 в UI-режиме).
    base_size: (u32, u32),
    /// Текущая раскладка кадра в окно (целочисленный масштаб + поля).
    frame_scale: FrameScale,
    /// Кэш курсоров: ключ (`CursorId | frame << 8`) → SDL-курсор.
    cursors: HashMap<u32, Cursor>,
    /// Какой ключ сейчас установлен (SDL-курсор живёт в этом кэше).
    active_cursor: Option<u32>,
    /// Контекст SDL держим последним: поля Rust удаляются в порядке
    /// объявления, поэтому окно/курсоры уничтожаются **до** `SDL_Quit`.
    _sdl: sdl3::Sdl,
}

impl SdlWindow {
    /// Создаёт окно `width × height` с заголовком `title`.
    pub fn new(title: &str, width: u32, height: u32) -> Result<Self, WindowError> {
        Self::new_scaled(title, width, height, Some(1))
    }

    /// Создаёт окно `width*scale × height*scale`; `scale = None` — подобрать
    /// целочисленный масштаб под рабочий стол (по умолчанию, пункт F).
    ///
    /// Окно ресайзится; [`Self::present`] каждый кадр пересчитывает
    /// [`FrameScale`] и выводит кадр nearest-neighbour с чёрными полями.
    pub fn new_scaled(
        title: &str,
        width: u32,
        height: u32,
        scale: Option<u32>,
    ) -> Result<Self, WindowError> {
        let width = width.max(1);
        let height = height.max(1);
        let sdl = sdl3::init().map_err(|error| WindowError::new("init", error))?;
        let video = sdl
            .video()
            .map_err(|error| WindowError::new("video", error))?;
        let scale = match scale {
            Some(scale) => scale.clamp(1, crate::frame_scale::MAX_SCALE),
            None => {
                let bounds = video
                    .get_primary_display()
                    .ok()
                    .and_then(|display| display.get_usable_bounds().ok());
                let auto = bounds
                    .map(|rect| (rect.width() / width).min(rect.height() / height))
                    .unwrap_or(1);

                auto.clamp(1, 4)
            }
        };
        let window_width = width * scale;
        let window_height = height * scale;
        let window = video
            .window(title, window_width, window_height)
            .resizable()
            .position_centered()
            .build()
            .map_err(|error| WindowError::new("window", error))?;
        let canvas: Canvas<Window> = window.into_canvas();
        let creator = canvas.texture_creator();
        let events = sdl
            .event_pump()
            .map_err(|error| WindowError::new("event_pump", error))?;
        let (actual_width, actual_height) = canvas.window().size();

        Ok(Self {
            canvas,
            creator,
            events,
            title: title.to_owned(),
            base_size: (width, height),
            frame_scale: FrameScale::fit(actual_width, actual_height, width, height),
            cursors: HashMap::new(),
            active_cursor: None,
            _sdl: sdl,
        })
    }

    /// Размер окна в пикселях.
    pub fn size(&self) -> (u32, u32) {
        self.canvas.window().size()
    }

    /// Логический размер кадра.
    pub fn base_size(&self) -> (u32, u32) {
        self.base_size
    }

    /// Текущий целочисленный масштаб кадра.
    pub fn frame_scale(&self) -> FrameScale {
        self.frame_scale
    }

    /// Пересчитывает раскладку под текущий размер окна и размер кадра.
    pub fn update_frame_scale(&mut self, frame_width: u32, frame_height: u32) -> FrameScale {
        let (window_width, window_height) = self.size();

        if (frame_width, frame_height) != self.frame_scale.frame_size()
            || (window_width, window_height) != self.frame_scale.window_size()
        {
            self.base_size = (frame_width, frame_height);
            self.frame_scale =
                FrameScale::fit(window_width, window_height, frame_width, frame_height);
        }

        self.frame_scale
    }

    /// Точка окна → координаты кадра (для кликов при масштабе ≠ 1).
    pub fn window_to_frame(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        self.frame_scale.window_to_frame(x, y)
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

    /// Ставит курсор из RGBA8-кадра с hotspot (`SDL_CreateColorCursor`).
    ///
    /// `key` — идентификатор кадра (`CursorId | frame << 8`): SDL-курсоры
    /// кэшируются, повторный вызов с тем же ключом ничего не делает. Так
    /// анимированный `PTR_PATH` (10 кадров) не пересоздаёт курсор каждый тик.
    /// Прозрачные пиксели (`alpha == 0`) остаются прозрачными.
    #[allow(clippy::too_many_arguments)]
    pub fn set_cursor_rgba(
        &mut self,
        key: u32,
        rgba: &[u8],
        width: u32,
        height: u32,
        hot_x: i32,
        hot_y: i32,
    ) -> Result<(), WindowError> {
        if self.active_cursor == Some(key) {
            return Ok(());
        }

        if !self.cursors.contains_key(&key) {
            if width == 0 || height == 0 {
                return Err(WindowError::new("cursor", "нулевой размер курсора"));
            }

            let expected = width as usize * height as usize * 4;

            if rgba.len() < expected {
                return Err(WindowError::new(
                    "cursor",
                    format!("кадр курсора короче: {} < {expected}", rgba.len()),
                ));
            }

            let mut surface = Surface::new(width, height, PixelFormat::RGBA32)
                .map_err(|error| WindowError::new("cursor_surface", error))?;
            let pitch = surface.pitch() as usize;
            let row_bytes = width as usize * 4;

            surface.with_lock_mut(|pixels: &mut [u8]| {
                for y in 0..height as usize {
                    let source = &rgba[y * row_bytes..(y + 1) * row_bytes];
                    let start = y * pitch;

                    if start + row_bytes <= pixels.len() {
                        pixels[start..start + row_bytes].copy_from_slice(source);
                    }
                }
            });

            let cursor = Cursor::from_surface(surface, hot_x, hot_y)
                .map_err(|error| WindowError::new("cursor_create", error))?;

            self.cursors.insert(key, cursor);
        }

        if let Some(cursor) = self.cursors.get(&key) {
            cursor.set();
            self.active_cursor = Some(key);
        }

        Ok(())
    }

    /// Выводит кадр: заливает текстуру и выводит её в окно nearest-neighbour.
    ///
    /// Кадр не растягивается на всё окно: [`FrameScale`] считает целый
    /// масштаб и центрирует его, а вокруг остаются чёрные поля. Так каждый
    /// пиксель кадра 640×480 остаётся квадратным блоком `N×N` на любом
    /// мониторе (пункт F), а мышь переводится обратно через
    /// [`Self::window_to_frame`].
    pub fn present(&mut self, buffer: &RgbaBuffer) -> Result<(), WindowError> {
        if buffer.is_empty() {
            return Ok(());
        }

        let scale = self.update_frame_scale(buffer.width(), buffer.height());
        let mut texture = self
            .creator
            .create_texture_streaming(PixelFormat::RGBA32, buffer.width(), buffer.height())
            .map_err(|error| WindowError::new("texture", error))?;

        texture.set_scale_mode(ScaleMode::Nearest);
        texture
            .update(None, buffer.pixels(), buffer.stride())
            .map_err(|error| WindowError::new("texture_update", error))?;

        self.canvas
            .set_draw_color(sdl3::pixels::Color::RGBA(0, 0, 0, 255));
        self.canvas.clear();

        let content = scale.content_rect();
        let destination = SdlRect::new(
            content.ulx,
            content.uly,
            content.width.max(1) as u32,
            content.height.max(1) as u32,
        );

        self.canvas
            .copy(&texture, None, destination)
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

    /// Опрашивает очередь событий и возвращает их UI-слою.
    ///
    /// Координаты мыши переводятся из окна в логический кадр через
    /// [`FrameScale`]: при масштабе 2 клик в окне 1280×960 приходит в UI как
    /// клик в кадре 640×480 (пункт F). Клик по чёрному полю вне кадра даёт
    /// `(-1, -1)` — UI его игнорирует.
    ///
    /// `running == false` означает закрытие окна (`Quit`); Esc **не** закрывает
    /// окно на этом уровне — UI решает сам (в меню — выход, в подэкране —
    /// назад). Для старого поведения «Esc закрывает окно» есть [`Self::poll`].
    pub fn poll_input(&mut self) -> (bool, Vec<WindowInput>) {
        let mut running = true;
        let mut inputs = Vec::new();
        // Окно могли ресайзить с прошлого кадра — обновляем раскладку до
        // пересчёта координат мыши.
        let (window_width, window_height) = self.size();
        self.frame_scale = FrameScale::fit(
            window_width,
            window_height,
            self.base_size.0,
            self.base_size.1,
        );
        let frame_scale = self.frame_scale;

        for event in self.events.poll_iter() {
            match event {
                sdl3::event::Event::Quit { .. } => running = false,
                sdl3::event::Event::KeyDown {
                    keycode: Some(code),
                    repeat: false,
                    ..
                } => inputs.push(WindowInput::Key(window_key(code))),
                sdl3::event::Event::MouseMotion { x, y, .. } => {
                    let (x, y) = frame_scale
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseMove { x, y });
                }
                sdl3::event::Event::MouseButtonDown {
                    mouse_btn: MouseButton::Left,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = frame_scale
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseClick { x, y });
                }
                sdl3::event::Event::MouseButtonDown {
                    mouse_btn: MouseButton::Right,
                    x,
                    y,
                    ..
                } => {
                    let (x, y) = frame_scale
                        .window_to_frame(x.round() as i32, y.round() as i32)
                        .unwrap_or((-1, -1));
                    inputs.push(WindowInput::MouseRightClick { x, y });
                }
                _ => {}
            }
        }

        (running, inputs)
    }

    /// Опрашивает очередь событий.
    ///
    /// Возвращает `false`, когда окно пора закрывать (закрытие окна или Esc).
    /// Это прежнее поведение `--connect --sdl`; UI-цикл использует
    /// [`Self::poll_input`], чтобы обрабатывать Esc как навигацию.
    pub fn poll(&mut self) -> bool {
        let (running, inputs) = self.poll_input();

        running
            && !inputs
                .iter()
                .any(|input| matches!(input, WindowInput::Key(WindowKey::Escape)))
    }
}
