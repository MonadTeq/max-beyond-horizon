//! Скролл карты по краям экрана — порт C++ `GameManager_ProcessInput`
//! (`game_manager.cpp:6351-6563`) и `GameManager_ScrollMainMapView`
//! (`:1652-1677`).
//!
//! # Оракул
//!
//! * `window_manager.cpp:90-101` — двенадцать окон `WINDOW_SCROLL_*` (8 px по
//!   краям кадра, 50×50 в углах). `GameManager_ProcessInput` выбирает **первое**
//!   окно под мышью в порядке таблицы `windows[]`, поэтому прямоугольники
//!   проверяются в том же порядке (углы раньше сторон, `End Turn`/`Files`/
//!   `Preferences` — раньше скролла, они в таблице выше);
//! * `GameManager_SetGridOffset` (`:6351`) — при `click_scroll=1` смещение
//!   принимается, только пока зажата ЛКМ (или идёт мультивыделение
//!   `RenderState == 2`); при `click_scroll=0` — по одному наведению;
//! * `GameManager_ScrollMainMapView` (`:1652`) — накопление
//!   `ScrollRate += GridStepLevel * ScrollRateLimit`, шаг — целая часть;
//!   `GridStepLevel = (MapScalingFactor * quick_scroll) / GFX_SCALE_DENOMINATOR`
//!   (`:1711-1718`), что в **экранных** пикселях ровно `quick_scroll`
//!   (перевод мир↔экран сокращается), поэтому здесь шаг — `quick_scroll` px;
//! * `GameManager_UpdateScrollRateLimit` (`:1636`) — ограничитель `1.5` кадра на
//!   случай подвисания.
//!
//! # Отличие ввода
//!
//! Оконный слой Rust-клиента (`max_render::sdl_window::poll_input`) отдаёт
//! только `MouseButtonDown`, без `MouseButtonUp`. Поэтому «зажатая ЛКМ»
//! моделируется импульсом [`ScrollState::press`] длиной
//! [`CLICK_HOLD_SECONDS`]: один клик по краю прокручивает карту на несколько
//! кадров, а не до отпускания. При `click_scroll=1` это единственный способ
//! скролла; при `click_scroll=0` работает чистое наведение, как в C++.

/// Кадров в секунду, к которым привязан `GridStepLevel` оригинала
/// (`SVGA_DEFAULT_REFRESH_RATE = 30`, `svga.cpp:32`).
pub const REFRESH_RATE: f32 = 30.0;

/// Максимум `ScrollRateLimit` (`game_manager.cpp:1643`).
pub const MAX_RATE_LIMIT: f32 = 1.5;

/// Сколько «держится» ЛКМ после клика (см. отличие ввода в шапке модуля).
pub const CLICK_HOLD_SECONDS: f32 = 0.12;

/// Одно из двенадцати окон прокрутки (`WINDOW_SCROLL_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollZone {
    /// `WINDOW_SCROLL_UP_WINDOW` — (50,0)…(590,8).
    Up,
    /// `WINDOW_SCROLL_UP_RIGHT_WINDOW` — (590,0)…(639,8).
    UpRight,
    /// `WINDOW_SCROLL_RIGHT_UP_WINDOW` — (632,0)…(639,50).
    RightUp,
    /// `WINDOW_SCROLL_RIGHT_WINDOW` — (632,50)…(639,430).
    Right,
    /// `WINDOW_SCROLL_RIGHT_DOWN_WINDOW` — (632,430)…(639,479).
    RightDown,
    /// `WINDOW_SCROLL_DOWN_RIGHT_WINDOW` — (590,472)…(639,479).
    DownRight,
    /// `WINDOW_SCROLL_DOWN_WINDOW` — (50,472)…(590,479).
    Down,
    /// `WINDOW_SCROLL_DOWN_LEFT_WINDOW` — (0,472)…(50,479).
    DownLeft,
    /// `WINDOW_SCROLL_LEFT_DOWN_WINDOW` — (0,430)…(8,479).
    LeftDown,
    /// `WINDOW_SCROLL_LEFT_WINDOW` — (0,50)…(8,430).
    Left,
    /// `WINDOW_SCROLL_LEFT_UP_WINDOW` — (0,0)…(8,50).
    LeftUp,
    /// `WINDOW_SCROLL_UP_LEFT_WINDOW` — (0,0)…(50,8).
    UpLeft,
}

impl ScrollZone {
    /// Все зоны в порядке таблицы `windows[]` (важно: пересечения углов и
    /// сторон разрешаются первым совпадением, как в `GameManager_ProcessInput`).
    pub const ALL: [Self; 12] = [
        Self::Up,
        Self::UpRight,
        Self::RightUp,
        Self::Right,
        Self::RightDown,
        Self::DownRight,
        Self::Down,
        Self::DownLeft,
        Self::LeftDown,
        Self::Left,
        Self::LeftUp,
        Self::UpLeft,
    ];

    /// Прямоугольник окна в кадре 640×480 — **включающие** координаты C++
    /// `WINDOW_RECT(ulx, uly, lrx, lry)`.
    pub const fn rect(self) -> (i32, i32, i32, i32) {
        match self {
            Self::Up => (50, 0, 590, 8),
            Self::UpRight => (590, 0, 639, 8),
            Self::RightUp => (632, 0, 639, 50),
            Self::Right => (632, 50, 639, 430),
            Self::RightDown => (632, 430, 639, 479),
            Self::DownRight => (590, 472, 639, 479),
            Self::Down => (50, 472, 590, 479),
            Self::DownLeft => (0, 472, 50, 479),
            Self::LeftDown => (0, 430, 8, 479),
            Self::Left => (0, 50, 8, 430),
            Self::LeftUp => (0, 0, 8, 50),
            Self::UpLeft => (0, 0, 50, 8),
        }
    }

    /// Точка внутри окна (включающие границы, как `WINDOW_RECT`).
    pub fn contains(self, x: i32, y: i32) -> bool {
        let (ulx, uly, lrx, lry) = self.rect();

        x >= ulx && x <= lrx && y >= uly && y <= lry
    }

    /// Направление прокрутки в клетках: `GameManager_SetGridOffset(±1, ±1)`.
    pub const fn direction(self) -> (i32, i32) {
        match self {
            Self::Up | Self::UpLeft | Self::RightUp => (0, -1),
            Self::UpRight => (1, -1),
            Self::Right => (1, 0),
            Self::RightDown | Self::DownRight => (1, 1),
            Self::Down => (0, 1),
            Self::DownLeft | Self::LeftDown => (-1, 1),
            Self::Left => (-1, 0),
            Self::LeftUp => (-1, -1),
        }
    }
}

/// Зона прокрутки под точкой окна (`None` — точка не в полосе скролла).
///
/// Порядок — как обход `windows[]` в `GameManager_ProcessInput`: первое
/// совпадение выигрывает, поэтому угловые окна перекрывают стороны ровно так
/// же, как в C++ (`UpRight` раньше `RightUp` и т. д.).
pub fn zone_at(x: i32, y: i32) -> Option<ScrollZone> {
    ScrollZone::ALL
        .into_iter()
        .find(|zone| zone.contains(x, y))
}

/// Состояние прокрутки: указатель, «зажатая» ЛКМ и накопитель шага.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrollState {
    pointer: Option<(i32, i32)>,
    hold: bool,
    hold_left: f32,
    rate: f32,
}

impl ScrollState {
    /// Пустое состояние (мышь вне кадра, кнопка не нажата).
    pub fn new() -> Self {
        Self::default()
    }

    /// Запоминает позицию указателя (`MouseMove`).
    pub fn set_pointer(&mut self, x: i32, y: i32) {
        self.pointer = Some((x, y));
    }

    /// Указатель покинул кадр (`MouseMove(-1,-1)` вне поля).
    pub fn clear_pointer(&mut self) {
        self.pointer = None;
    }

    /// Клик ЛКМ: начинает импульс «удержания» (`click_scroll`).
    pub fn press(&mut self, x: i32, y: i32) {
        self.pointer = Some((x, y));
        self.hold = true;
        self.hold_left = CLICK_HOLD_SECONDS;
    }

    /// Отпускание ЛКМ (если появится в оконном слое).
    pub fn release(&mut self) {
        self.hold = false;
        self.hold_left = 0.0;
    }

    /// Считает импульс удержания (`dt` секунд).
    pub fn tick(&mut self, dt: f32) {
        if self.hold {
            self.hold_left -= dt.max(0.0);

            if self.hold_left <= 0.0 {
                self.hold = false;
                self.hold_left = 0.0;
            }
        }
    }

    /// ЛКМ считается зажатой (клик ещё «держится»).
    pub fn is_held(&self) -> bool {
        self.hold
    }

    /// Текущий указатель.
    pub fn pointer(&self) -> Option<(i32, i32)> {
        self.pointer
    }

    /// Зона под указателем.
    pub fn zone(&self) -> Option<ScrollZone> {
        self.pointer.and_then(|(x, y)| zone_at(x, y))
    }

    /// Один шаг прокрутки: `Some((dx, dy))` в **экранных** пикселях.
    ///
    /// `click_scroll` — настройка «Click to Scroll»: при `true` прокрутка идёт
    /// только при зажатой ЛКМ, при `false` — по наведению. `quick_scroll` —
    /// «Scroll Speed» (4…128). Шаг накапливается как
    /// `GameManager_ScrollMainMapView`: дробная часть переносится на следующий
    /// кадр, поэтому скорость не зависит от частоты кадров.
    pub fn step(
        &mut self,
        dt: f32,
        click_scroll: bool,
        quick_scroll: i32,
    ) -> Option<(i32, i32)> {
        let (x, y) = self.pointer?;
        let Some(zone) = active_zone_at(x, y) else {
            self.rate = 0.0;
            return None;
        };

        if click_scroll && !self.hold {
            self.rate = 0.0;
            return None;
        }

        let rate_limit = if dt <= 0.0 {
            0.0
        } else {
            (dt * REFRESH_RATE).min(MAX_RATE_LIMIT)
        };

        self.rate += quick_scroll.clamp(1, 128) as f32 * rate_limit;

        let step = self.rate.floor() as i32;

        if step <= 0 {
            return None;
        }

        self.rate -= step as f32;

        let (dx, dy) = zone.direction();

        Some((dx * step, dy * step))
    }

    /// Сбрасывает накопитель (смена зоны/остановка).
    pub fn reset_rate(&mut self) {
        self.rate = 0.0;
    }
}

/// Прямоугольник полосы прокрутки в координатах `PixelRect` (для отрисовки/
/// тестов): `(ulx, uly, width, height)`.
pub fn zone_rect(zone: ScrollZone) -> (i32, i32, i32, i32) {
    let (ulx, uly, lrx, lry) = zone.rect();

    (ulx, uly, lrx - ulx + 1, lry - uly + 1)
}

/// Полоса прокрутки не должна перекрывать кликабельные кнопки панели: в C++
/// `WINDOW_ENDTURN_BUTTON`/`FILES`/`PREFS` стоят в таблице раньше скролла,
/// поэтому `GameManager_ProcessInput` отдаёт клик им. Проверка вынесена сюда,
/// чтобы её использовал и клиент (`App`), и тесты.
pub fn panel_button_blocks(x: i32, y: i32) -> bool {
    crate::panels::panel_button_at(x, y).is_some()
}

/// Точка в полосе прокрутки с учётом приоритета кнопок панели.
pub fn active_zone_at(x: i32, y: i32) -> Option<ScrollZone> {
    if panel_button_blocks(x, y) {
        return None;
    }

    zone_at(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_match_the_cpp_window_table() {
        // Середины сторон и диагональные сектора.
        assert_eq!(zone_at(300, 4), Some(ScrollZone::Up));
        assert_eq!(zone_at(600, 4), Some(ScrollZone::UpRight));
        assert_eq!(zone_at(636, 20), Some(ScrollZone::RightUp));
        assert_eq!(zone_at(636, 200), Some(ScrollZone::Right));
        assert_eq!(zone_at(636, 460), Some(ScrollZone::RightDown));
        assert_eq!(zone_at(600, 476), Some(ScrollZone::DownRight));
        assert_eq!(zone_at(300, 476), Some(ScrollZone::Down));
        assert_eq!(zone_at(20, 476), Some(ScrollZone::DownLeft));
        assert_eq!(zone_at(4, 460), Some(ScrollZone::LeftDown));
        assert_eq!(zone_at(4, 200), Some(ScrollZone::Left));
        assert_eq!(zone_at(4, 20), Some(ScrollZone::LeftUp));
        assert_eq!(zone_at(20, 4), Some(ScrollZone::UpLeft));
    }

    #[test]
    fn map_and_panel_interior_are_not_scroll_zones() {
        assert_eq!(zone_at(300, 200), None);
        assert_eq!(zone_at(100, 100), None);
        assert_eq!(zone_at(300, 30), None);
        // End Turn заходит на карту, но клик всё равно принадлежит кнопке
        // (`WINDOW_ENDTURN_BUTTON` раньше `WINDOW_SCROLL_UP_WINDOW` в C++).
        assert_eq!(zone_at(400, 4), Some(ScrollZone::Up));
        assert_eq!(active_zone_at(400, 4), None);
        assert_eq!(active_zone_at(300, 4), Some(ScrollZone::Up));
    }

    #[test]
    fn directions_match_set_grid_offset() {
        assert_eq!(ScrollZone::Up.direction(), (0, -1));
        assert_eq!(ScrollZone::UpRight.direction(), (1, -1));
        assert_eq!(ScrollZone::Right.direction(), (1, 0));
        assert_eq!(ScrollZone::DownRight.direction(), (1, 1));
        assert_eq!(ScrollZone::Down.direction(), (0, 1));
        assert_eq!(ScrollZone::DownLeft.direction(), (-1, 1));
        assert_eq!(ScrollZone::Left.direction(), (-1, 0));
        assert_eq!(ScrollZone::LeftUp.direction(), (-1, -1));
    }

    #[test]
    fn click_scroll_requires_a_held_button() {
        let mut scroll = ScrollState::new();
        scroll.set_pointer(300, 4);

        assert_eq!(scroll.step(1.0 / 30.0, true, 16), None);
        assert!(scroll.step(1.0 / 30.0, false, 16).is_some());

        scroll.press(300, 4);
        assert_eq!(
            scroll.step(1.0 / 30.0, true, 16),
            Some((0, -16)),
            "клик по верхней полосе прокручивает карту вверх"
        );
    }

    #[test]
    fn quick_scroll_sets_the_step_size() {
        let mut scroll = ScrollState::new();
        scroll.set_pointer(636, 200);

        assert_eq!(scroll.step(1.0 / 30.0, false, 16), Some((16, 0)));

        scroll.reset_rate();
        assert_eq!(scroll.step(1.0 / 30.0, false, 64), Some((64, 0)));
    }

    #[test]
    fn fractional_rate_carries_over_between_frames() {
        let mut scroll = ScrollState::new();
        scroll.set_pointer(300, 4);

        // «Scroll Speed» 1 при 60 Гц: 1 * (1/60 * 30) = 0.5 px за кадр —
        // целого шага ещё нет, дробная часть переносится.
        assert_eq!(scroll.step(1.0 / 60.0, false, 1), None);

        // Ещё полкадра — накопился ровно 1 px.
        assert_eq!(scroll.step(1.0 / 60.0, false, 1), Some((0, -1)));
    }

    #[test]
    fn dt_zero_does_not_scroll() {
        let mut scroll = ScrollState::new();
        scroll.set_pointer(300, 4);

        assert_eq!(scroll.step(0.0, false, 128), None);
    }

    #[test]
    fn hold_expires_after_the_pulse() {
        let mut scroll = ScrollState::new();
        scroll.press(300, 4);
        assert!(scroll.is_held());

        scroll.tick(CLICK_HOLD_SECONDS);

        assert!(!scroll.is_held());
        assert_eq!(scroll.step(1.0 / 30.0, true, 16), None);
    }

    #[test]
    fn zone_rects_are_the_cpp_inclusive_rects() {
        assert_eq!(zone_rect(ScrollZone::Up), (50, 0, 541, 9));
        assert_eq!(zone_rect(ScrollZone::Right), (632, 50, 8, 381));
        assert_eq!(zone_rect(ScrollZone::UpLeft), (0, 0, 51, 9));
    }
}
