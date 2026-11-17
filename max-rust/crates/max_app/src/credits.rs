//! Экран `Credits` главного меню — построчный порт `menu_credits_menu_loop`
//! (`max-cpp-port/src/menu.cpp:1332`).
//!
//! Что делает оригинал:
//!
//! 1. грузит big image `CREDITS` 640×480 (`palette_from_image = true`, то есть
//!    системная палитра — палитра картинки) и показывает её как фон;
//! 2. выделяет центральную область `450×230` (`WindowManager_ScaleUlx/Uly`,
//!    при окне 640×480 — ровно 450×230): `ulx = (640-450)/2 = 95`,
//!    `uly = (480-230)/2 = 125`;
//! 3. держит «буфер прокрутки» высотой `230+18 = 248` строк: каждый шаг
//!    сдвигает его на 1 пиксель вверх, а когда `height_offset <= 355`
//!    (низ области) — рисует следующую строку титров шрифтом `FONT1` в строку
//!    `231` (`Text_TextBox(…, uly = 230, height = 0x12 = 18, outline)`:
//!    `font_height = 16+1 = 17`, `uly += ((18+1) - 17)/2 = 1` → 231) и
//!    сбрасывает `height_offset = 355 + 20 = 375`;
//! 4. шаг длится `TIMER_FPS_TO_MS(scroll_speed)` = `1000/100 = 10 мс`;
//!    `KP-`/`KP+` меняют скорость на 9 (в пределах 9…1000), любая другая
//!    клавиша — выход;
//! 5. когда `height_offset <= 125` (верх области) — экран закрывается.
//!
//! Таблица строк — `data/credits.json` (генератор
//! `max-rust/tools/gen_credits_data.py`): 171 строка, цвет `0xFF` у заголовков
//! и `0x04` у имён; `CREDITS_SEPARATOR` — пустая строка, она ничего не рисует.
//!
//! # Детерминизм
//!
//! Состояние — чистая функция от числа шагов: строка `k` нарисована на шаге
//! `20·k` и в шаге `t` лежит в строке `231 - (t - 20·k)`. Никакого буфера
//! пикселей и никакой зависимости от FPS: в QA `dt = 1/60`
//! ([`Self::advance`]), и кадр полностью определяется числом шагов.
//!
//! # Отличия от C++
//!
//! * Текст рисуется растровым GNW-шрифтом `FONT1` (как весь Rust-клиент), а не
//!   FreeType-ом; обводка — «тень» на +1 пиксель вправо/вниз индексом `0x00`
//!   (`Text_BlitTTF` рисует её цветом `Color_RGB2Color(0)`).
//! * `KP-`/`KP+` в клиенте недоступны (нет событий numpad): скорость меняется
//!   клавишами `-`/`+`.

use std::sync::OnceLock;

use max_assets::image::BigImage;
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered_shadowed, GnwFontId};
use serde::Deserialize;

use crate::panels::UiPalette;

/// JSON с таблицей титров (сгенерирован из `menu.cpp` + `language.json`).
pub const CREDITS_LINES_JSON: &str = include_str!("../data/credits.json");

/// Ширина области прокрутки (`WindowManager_ScaleUlx(window, 450)`).
pub const CREDITS_WIDTH: i32 = 450;
/// Высота области прокрутки (`WindowManager_ScaleUly(window, 230)`).
pub const CREDITS_HEIGHT: i32 = 230;
/// Левый край области: `(640-450)/2`.
pub const CREDITS_ULX: i32 = 95;
/// Верхний край области: `(480-230)/2`.
pub const CREDITS_ULY: i32 = 125;
/// Низ области = `CREDITS_ULY + CREDITS_HEIGHT` (в C++ — `bounds.lry`).
pub const CREDITS_BOTTOM: i32 = CREDITS_ULY + CREDITS_HEIGHT;
/// Строка буфера, куда рисуется новая строка (`window_height + 1`).
pub const TEXT_ROW: i32 = CREDITS_HEIGHT + 1;
/// Через сколько шагов появляется следующая строка (`bounds.lry + 20`).
pub const LINE_STEP: i32 = 20;
/// Шаг изменения скорости (`KP-`/`KP+`).
pub const SPEED_RATE: u32 = 9;
/// Скорость по умолчанию: `TIMER_FPS_TO_MS(100)` = 10 мс на шаг.
pub const DEFAULT_SPEED: u32 = 100;
/// Максимальная скорость (как в C++: `scroll_speed < 1000`).
pub const MAX_SPEED: u32 = 1000;
/// Сколько шагов максимум делает [`CreditsScreen::advance`] за кадр.
///
/// Защита от «наверстывания» после паузы: без неё один кадр с `dt = 10 с`
/// прокрутил бы весь экран за раз, но детерминизм бы сохранился. Предел
/// держит кадр ограниченным по времени.
pub const MAX_STEPS_PER_FRAME: u32 = 600;

/// Одна строка титров.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CreditsLine {
    /// Индекс цвета в палитре `CREDITS` (`0xFF` — заголовок, `0x04` — текст).
    pub color: u8,
    /// Текст (en-US).
    pub text: String,
}

#[derive(Debug, Deserialize)]
struct CreditsFile {
    lines: Vec<CreditsLine>,
}

/// Таблица титров (`menu_credits_lines[]`).
pub fn lines() -> &'static [CreditsLine] {
    static LINES: OnceLock<Vec<CreditsLine>> = OnceLock::new();

    LINES
        .get_or_init(|| {
            serde_json::from_str::<CreditsFile>(CREDITS_LINES_JSON)
                .map(|file| file.lines)
                .unwrap_or_default()
        })
        .as_slice()
}

/// Сколько строк титров (171 в оригинале).
pub fn line_count() -> usize {
    lines().len()
}

/// Состояние прокрутки титров (эквивалент локальных переменных
/// `menu_credits_menu_loop`: `line_index`, `height_offset`, `scroll_speed`,
/// `play`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditsScreen {
    steps: u32,
    line_index: usize,
    height_offset: i32,
    scroll_speed: u32,
    elapsed_us: u64,
    finished: bool,
}

impl Default for CreditsScreen {
    fn default() -> Self {
        Self {
            steps: 0,
            line_index: 0,
            height_offset: CREDITS_BOTTOM,
            scroll_speed: DEFAULT_SPEED,
            elapsed_us: 0,
            finished: false,
        }
    }
}

impl CreditsScreen {
    /// Новый экран: первая строка появится на первом же шаге.
    pub fn new() -> Self {
        Self::default()
    }

    /// Сколько шагов прокрутки сделано.
    #[inline]
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// Сколько строк уже показано.
    #[inline]
    pub fn line_index(&self) -> usize {
        self.line_index
    }

    /// Текущий `height_offset` (для тестов).
    #[inline]
    pub fn height_offset(&self) -> i32 {
        self.height_offset
    }

    /// Текущая скорость (шагов в секунду; 100 — как в оригинале).
    #[inline]
    pub fn scroll_speed(&self) -> u32 {
        self.scroll_speed
    }

    /// Прокрутка дошла до верха области (`height_offset <= bounds.uly`).
    #[inline]
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// `KP-`: медленнее (`scroll_speed -= 9`, пока `>= 9`).
    pub fn slower(&mut self) {
        if self.scroll_speed >= SPEED_RATE {
            self.scroll_speed -= SPEED_RATE;
        }
    }

    /// `KP+`: быстрее (`scroll_speed += 9`, пока `< 1000`).
    pub fn faster(&mut self) {
        if self.scroll_speed < MAX_SPEED {
            self.scroll_speed += SPEED_RATE;
        }
    }

    /// Один шаг прокрутки (`while (play)` в C++ без ожидания).
    fn step(&mut self) {
        self.steps = self.steps.saturating_add(1);
        self.height_offset -= 1;

        if self.line_index < lines().len() && self.height_offset <= CREDITS_BOTTOM {
            self.line_index += 1;
            self.height_offset = CREDITS_BOTTOM + LINE_STEP;
        }

        if self.height_offset <= CREDITS_ULY {
            self.finished = true;
        }
    }

    /// Продвигает прокрутку на `dt` секунд (`dt <= 0` — стоит, как портрет).
    pub fn advance(&mut self, dt: f32) {
        if self.finished || !(dt > 0.0) {
            return;
        }

        let micros = (f64::from(dt) * 1_000_000.0).round() as u64;

        self.elapsed_us = self.elapsed_us.saturating_add(micros);

        let frame_us = 1_000_000 / u64::from(self.scroll_speed.max(1));
        let mut budget = MAX_STEPS_PER_FRAME;

        while self.elapsed_us >= frame_us && !self.finished && budget > 0 {
            self.elapsed_us -= frame_us;
            self.step();
            budget -= 1;
        }

        if budget == 0 {
            // Не копим долг: следующий кадр начнёт с чистого листа.
            self.elapsed_us = 0;
        }
    }

    /// Строка буфера, в которой сейчас находится строка титров `index`
    /// (может быть отрицательной — строка уже ушла вверх).
    pub fn row_of(&self, index: usize) -> i32 {
        // Строка `k` нарисована на шаге `1 + 20·k` (первый шаг рисует строку 0
        // и сбрасывает `height_offset` в 375, следующие 20 шагов опускают его
        // до 355); с тех пор буфер уехал вверх на `steps - (1 + 20k)`.
        let drawn_at = 1 + (index as i32).saturating_mul(LINE_STEP);

        TEXT_ROW - (self.steps as i32 - drawn_at)
    }

    /// Видимые строки: `(номер строки, строка буфера)`.
    pub fn visible_lines(&self) -> Vec<(usize, i32)> {
        let font = GnwFontId::Font1.font();
        let height = font.height();
        let mut visible = Vec::new();

        for index in 0..self.line_index {
            let row = self.row_of(index);

            if row < CREDITS_HEIGHT && row + height > 0 {
                visible.push((index, row));
            }
        }

        visible
    }

    /// Рисует экран: фон `CREDITS` + видимые строки титров в области 450×230.
    ///
    /// Возвращает число закрашенных пикселей (для статистики рендера).
    pub fn draw(&self, frame: &mut RgbaBuffer, background: &BigImage) -> u32 {
        let painted = crate::menu::draw_big_image(frame, background);
        let palette = UiPalette::from_big_image(background);
        let font = GnwFontId::Font1.font();
        let outline = palette.rgba(0x00);

        // Текст рисуется в отдельный буфер области (прозрачный фон), чтобы
        // строки за границей 450×230 не залезали на фон вокруг области —
        // ровно как обрезка буфера прокрутки в C++.
        let mut region = RgbaBuffer::new(CREDITS_WIDTH as u32, CREDITS_HEIGHT as u32);

        for (index, row) in self.visible_lines() {
            let line = &lines()[index];

            if line.text.is_empty() {
                continue;
            }

            let rect = PixelRect::new(0, row - 1, CREDITS_WIDTH, 18);

            draw_text_gnw_centered_shadowed(
                &mut region,
                rect,
                font,
                &line.text,
                palette.rgba(line.color),
                outline,
                outline,
            );
        }

        let mut additional = 0;

        for y in 0..CREDITS_HEIGHT {
            for x in 0..CREDITS_WIDTH {
                let Some(color) = region.get(x, y) else {
                    continue;
                };

                if color.a == 0 {
                    continue;
                }

                if frame.set(CREDITS_ULX + x, CREDITS_ULY + y, color) {
                    additional += 1;
                }
            }
        }

        painted + additional
    }
}

/// Цвет фона/текста для тестов: прозрачность не должна протекать в кадр.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_has_171_lines_in_menu_cpp_order() {
        assert_eq!(line_count(), 171, "menu_credits_lines: 171 непустая строка");
        assert_eq!(lines()[0].text, "Project Manager");
        assert_eq!(lines()[0].color, 0xFF);
        assert_eq!(lines()[1].text, "Ali Atabek");
        assert_eq!(lines()[1].color, 0x04);
        assert_eq!(lines()[2].text, "Producers");
        // Благодарности в конце таблицы (Parallax Software — установщик).
        assert!(lines().iter().any(|line| line.text.contains("Parallax")));
        assert!(lines().iter().all(|line| !line.text.is_empty()));
    }

    #[test]
    fn dt_zero_keeps_the_screen_still() {
        let mut screen = CreditsScreen::new();

        for _ in 0..600 {
            screen.advance(0.0);
        }

        assert_eq!(screen.steps(), 0);
        assert_eq!(screen.line_index(), 0);
        assert!(!screen.finished());
        assert!(screen.visible_lines().is_empty());
    }

    #[test]
    fn one_second_scrolls_one_hundred_steps_at_default_speed() {
        let mut screen = CreditsScreen::new();

        for _ in 0..60 {
            screen.advance(1.0 / 60.0);
        }

        // 10 мс на шаг: за секунду — 99-100 шагов (округление микросекунд).
        assert!(
            matches!(screen.steps(), 99 | 100),
            "шагов: {}",
            screen.steps()
        );
        // За 100 шагов появляется 6 строк (шаг 20) = line_index 6.
        assert!(
            matches!(screen.line_index(), 5 | 6),
            "{}",
            screen.line_index()
        );
    }

    #[test]
    fn lines_appear_every_twenty_steps_and_scroll_up() {
        let mut screen = CreditsScreen::new();

        // Шаг 1: первая строка нарисована в строке 231 (ещё под окном).
        screen.advance(0.01);

        assert_eq!(screen.steps(), 1);
        assert_eq!(screen.line_index(), 1);
        assert_eq!(screen.row_of(0), TEXT_ROW);
        assert!(screen.visible_lines().is_empty());

        // Шаг 2: строка в 230 — ещё не влезает в окно (видны строки 0..229).
        screen.advance(0.01);
        assert_eq!(screen.row_of(0), TEXT_ROW - 1);
        assert!(screen.visible_lines().is_empty());

        // Шаг 3: строка на 229 — нижняя видимая строка окна.
        screen.advance(0.01);
        assert_eq!(screen.visible_lines(), vec![(0, TEXT_ROW - 2)]);

        // К шагу 21 добавлена вторая строка, первая уехала на 20 вверх.
        for _ in 0..18 {
            screen.advance(0.01);
        }

        assert_eq!(screen.steps(), 21);
        assert_eq!(screen.line_index(), 2);
        assert_eq!(screen.row_of(0), TEXT_ROW - 20);
        assert_eq!(screen.row_of(1), TEXT_ROW);
        // Вторая строка ещё не влезла в окно (строка 231 > 229).
        assert_eq!(screen.visible_lines(), vec![(0, TEXT_ROW - 20)]);

        for _ in 0..20 {
            screen.advance(0.01);
        }

        assert_eq!(screen.steps(), 41);
        assert_eq!(screen.row_of(2), TEXT_ROW);
        // Третья строка ещё под окном — видны только первые две.
        assert_eq!(
            screen.visible_lines(),
            vec![(0, TEXT_ROW - 40), (1, TEXT_ROW - 20)]
        );
    }

    #[test]
    fn screen_finishes_after_the_last_line_leaves_the_top() {
        let mut screen = CreditsScreen::new();
        // Последняя строка рисуется на шаге `1 + 20·(N-1)`, затем 250 шагов,
        // пока `height_offset` не дойдёт от 375 до 125 (`bounds.uly`).
        let total = 1 + 20 * (line_count() as u32 - 1) + 250;

        // Проходим до конца: 3651 шаг при 100 шагах/с.
        for _ in 0..(total * 11) {
            screen.advance(0.01);
        }

        assert!(screen.finished(), "экран должен закрыться сам");
        assert_eq!(screen.line_index(), line_count());
        assert!(screen.height_offset() <= CREDITS_ULY);
        assert!(
            screen.steps() >= total,
            "шагов {} < {total}",
            screen.steps()
        );
        // После закрытия шаги не идут.
        let steps = screen.steps();
        screen.advance(1.0);
        assert_eq!(screen.steps(), steps);
    }

    #[test]
    fn speed_changes_by_nine_like_the_keypad_controls() {
        let mut screen = CreditsScreen::new();

        assert_eq!(screen.scroll_speed(), 100);

        screen.faster();
        assert_eq!(screen.scroll_speed(), 109);

        for _ in 0..200 {
            screen.faster();
        }
        assert_eq!(screen.scroll_speed(), MAX_SPEED);

        screen.slower();
        assert_eq!(screen.scroll_speed(), MAX_SPEED - SPEED_RATE);

        for _ in 0..200 {
            screen.slower();
        }
        // Останавливается на первой скорости меньше шага (как в C++:
        // `if (scroll_speed >= scroll_rate) scroll_speed -= scroll_rate`).
        assert!(
            screen.scroll_speed() < SPEED_RATE,
            "{}",
            screen.scroll_speed()
        );
        assert!(screen.scroll_speed() > 0);

        // Скорость влияет на прокрутку: 109 шагов/с → 1 шаг за 10 мс.
        let mut fast = CreditsScreen::new();
        fast.faster();
        fast.advance(0.01);
        assert_eq!(fast.steps(), 1);
    }

    #[test]
    fn draw_paints_background_and_text_only_inside_the_region() {
        let menu =
            crate::menu::MenuAssets::load(&max_assets::ResArchive::open(repo_max_res()).unwrap());

        let Some(menu) = menu else {
            eprintln!("пропуск: нет MAX.RES");
            return;
        };

        let Some(background) = menu.credits.as_ref() else {
            eprintln!("пропуск: нет CREDITS");
            return;
        };

        let mut screen = CreditsScreen::new();
        let mut frame = RgbaBuffer::new(640, 480);
        let painted = screen.draw(&mut frame, background);

        assert!(painted >= 640 * 480, "фон должен занять весь кадр");

        // В самом начале текст ещё не виден — область совпадает с фоном.
        let area = |frame: &RgbaBuffer, y0: i32, y1: i32| {
            let mut out = Vec::new();

            for y in y0..y1 {
                for x in CREDITS_ULX..(CREDITS_ULX + CREDITS_WIDTH) {
                    out.push(frame.get(x, y));
                }
            }

            out
        };

        let before = area(&frame, CREDITS_ULY, CREDITS_ULY + CREDITS_HEIGHT);

        for _ in 0..30 {
            screen.advance(0.01);
        }

        let mut with_text = RgbaBuffer::new(640, 480);
        screen.draw(&mut with_text, background);
        let after = area(&with_text, CREDITS_ULY, CREDITS_ULY + CREDITS_HEIGHT);

        assert_ne!(before, after, "строки титров должны изменить область");

        // За областью ничего не меняется.
        for y in 0..480 {
            for x in 0..640 {
                let inside = (CREDITS_ULX..CREDITS_ULX + CREDITS_WIDTH).contains(&x)
                    && (CREDITS_ULY..CREDITS_ULY + CREDITS_HEIGHT).contains(&y);

                if !inside {
                    assert_eq!(
                        frame.get(x, y),
                        with_text.get(x, y),
                        "пиксель вне области ({x},{y}) изменился"
                    );
                }
            }
        }
    }

    fn repo_max_res() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("max-original")
            .join("MAX.RES")
    }
}
