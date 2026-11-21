//! Самоуничтожение здания — порт `UnitsManager_SelfDestructMenu`
//! (`units_manager.cpp:292-357`) и `UnitsManager_SelfDestructActiveMenu`
//! (`:233-290`).
//!
//! # Оракул
//!
//! Окно `SELFDSTR` (174×82) по центру области карты
//! (`Window(SELFDSTR, GameManager_GetDialogWindowCenterMode())` →
//! `(180 + (448-174)/2, 18 + (448-82)/2) = (317, 201)`).
//!
//! * `Arm` — `Button(SLFDAR_U, SLFDAR_D, 89, 14)`, подпись `_(d5cb)` = "Arm",
//!   sfx `MBUTT0`;
//! * `Cancel` — `Button(SLFDCN_U, SLFDCN_D, 89, 46)`, подпись `_(ef77)` =
//!   "Cancel", sfx `NCANC0`, `Esc`;
//! * `Enter` нажимает `Arm` (`SetPValue(GNW_KB_KEY_RETURN)` /
//!   `SetRValue`), после чего C++ проигрывает кадры `SLFDOPN1..6` по
//!   `TIMER_FPS_TO_MS(48)` и показывает кнопку `SLFDOK_U/D` (61×58 в (13,11),
//!   `SetRValue(GNW_KB_KEY_RETURN)`, sfx `NDONE0`);
//! * подтверждение (`Enter`/клик по `OK`) возвращает `true`, отмена — `false`;
//!   `Esc` в любом окне — отмена (`NCANC0`).
//!
//! # Что портировано
//!
//! Оба окна и их ввод; анимация `SLFDOPN1..6` идёт по визуальным часам
//! (`App::update_visual_units`, шаг 1/60 с), а не по `SDL_Delay`, поэтому кадры
//! детерминированы (golden/QA). Приказ — `ORDER_EXPLODE`/`ORDER_STATE_EXPLODE`
//! (`UnitsManager_Popup_OnClick_Remove`, `units_manager.cpp:506`).
//!
//! # Осознанные отличия
//!
//! * C++ блокирует поток на 6×48 мс внутри `SelfDestructActiveMenu`; порт
//!   держит окно открытым и принимает `Esc`/`OK` после анимации (как игрок и
//!   ожидает), а не «проглатывает» ввод.
//! * Второе окно (`GNW_INPUT_PRESS`) в C++ только подсвечивает кнопку; звук
//!   отпускания здесь не моделируется.

use max_assets::image::{BigImage, SimpleImage};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, GnwFontId};

use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};
use max_core::unit::UnitId;

/// Окно `SELFDSTR`: 174×82 (`MAX.RES`).
pub const DIALOG_SIZE: (i32, i32) = (174, 82);
/// Окно в кадре — центр `WINDOW_MAIN_MAP` (180,18,448,448).
pub const DIALOG_RECT: PixelRect = PixelRect::new(317, 201, 174, 82);
/// `Arm` (`SLFDAR_U/D`, 74×23) в (89,14).
pub const ARM_RECT: PixelRect = PixelRect::new(89, 14, 74, 23);
/// `Cancel` (`SLFDCN_U/D`, 74×23) в (89,46).
pub const CANCEL_RECT: PixelRect = PixelRect::new(89, 46, 74, 23);
/// `OK` (`SLFDOK_U/D`, 61×58) в (13,11).
pub const OK_RECT: PixelRect = PixelRect::new(13, 11, 61, 58);
/// Сколько кадров `SLFDOPN1..6` (`units_manager.cpp:239`).
pub const OPEN_FRAMES: usize = 6;
/// `TIMER_FPS_TO_MS(48)` — шаг кадра.
pub const FRAME_SECONDS: f32 = 0.048;

/// Стадия диалога.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Первое окно: `Arm`/`Cancel`.
    Confirm,
    /// Второе окно: анимация `SLFDOPN` и `OK`.
    Armed,
}

/// Что делать с вводом.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfDestructAction {
    /// Окно остаётся открытым.
    None,
    /// Отмена: приказа нет.
    Cancel,
    /// Подтверждение: `ORDER_EXPLODE`.
    Explode,
}

/// Открытый диалог самоуничтожения.
#[derive(Debug, Clone, PartialEq)]
pub struct SelfDestructMenu {
    /// Здание, которое уничтожается.
    pub unit: UnitId,
    /// Стадия.
    pub stage: Stage,
    /// Номер кадра анимации `SLFDOPN1..6` (0..=6; 6 — кнопка `OK`).
    pub frame: usize,
    /// Накопленное время кадра.
    elapsed: f32,
}

impl SelfDestructMenu {
    /// Открывает первое окно (`UnitsManager_SelfDestructMenu`).
    pub fn new(unit: UnitId) -> Self {
        Self {
            unit,
            stage: Stage::Confirm,
            frame: 0,
            elapsed: 0.0,
        }
    }

    /// Имя картинки кадра анимации (`SLFDOPN1..6`).
    pub fn frame_image(&self) -> &'static str {
        match self.frame {
            0 => "SLFDOPN1",
            1 => "SLFDOPN2",
            2 => "SLFDOPN3",
            3 => "SLFDOPN4",
            4 => "SLFDOPN5",
            _ => "SLFDOPN6",
        }
    }

    /// Анимация доиграла — показывается `OK`.
    pub fn is_armed(&self) -> bool {
        self.stage == Stage::Armed && self.frame + 1 >= OPEN_FRAMES
    }

    /// Прямоугольники кнопок в кадре.
    pub fn arm_rect(&self) -> PixelRect {
        rect_at(ARM_RECT)
    }

    /// `Cancel`.
    pub fn cancel_rect(&self) -> PixelRect {
        rect_at(CANCEL_RECT)
    }

    /// `OK` второго окна.
    pub fn ok_rect(&self) -> PixelRect {
        rect_at(OK_RECT)
    }

    /// Визуальные часы (`TIMER_FPS_TO_MS(48)` на кадр).
    pub fn advance(&mut self, dt: f32) {
        if self.stage != Stage::Armed || self.is_armed() {
            return;
        }

        self.elapsed += dt.max(0.0);

        while self.elapsed >= FRAME_SECONDS && self.frame + 1 < OPEN_FRAMES {
            self.elapsed -= FRAME_SECONDS;
            self.frame += 1;
        }
    }

    /// Нажатие `Arm`/`Enter`.
    pub fn arm(&mut self) {
        self.stage = Stage::Armed;
        self.frame = 0;
        self.elapsed = 0.0;
    }

    /// Ввод: клик по кнопке или клавиша.
    pub fn input(&mut self, event: InputEvent) -> SelfDestructAction {
        match event {
            InputEvent::MouseClick(x, y) => {
                if self.stage == Stage::Confirm {
                    if self.arm_rect().contains(x, y) {
                        self.arm();
                        SelfDestructAction::None
                    } else if self.cancel_rect().contains(x, y) {
                        SelfDestructAction::Cancel
                    } else {
                        SelfDestructAction::None
                    }
                } else if self.is_armed() {
                    if self.ok_rect().contains(x, y) {
                        SelfDestructAction::Explode
                    } else {
                        SelfDestructAction::None
                    }
                } else {
                    // Анимация `SLFDOPN1..6` ещё играет — ввод игнорируется
                    // (C++ крутит цикл кадров до появления `OK`).
                    SelfDestructAction::None
                }
            }
            InputEvent::Key(Key::Enter) => {
                if self.stage == Stage::Confirm {
                    self.arm();
                    SelfDestructAction::None
                } else if self.is_armed() {
                    SelfDestructAction::Explode
                } else {
                    SelfDestructAction::None
                }
            }
            InputEvent::Key(Key::Escape) => SelfDestructAction::Cancel,
            _ => SelfDestructAction::None,
        }
    }

    /// Рисует окно (`Window(SELFDSTR)` + кнопки).
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        assets: Option<&SelfDestructAssets>,
        fallback: Option<UiPalette>,
    ) -> u32 {
        let palette = assets
            .and_then(|assets| assets.background.as_ref())
            .map(UiPalette::from_big_image)
            .or(fallback);

        let Some(palette) = palette else {
            return 0;
        };
        let window = DIALOG_RECT;
        let mut painted = 0;

        if let Some(background) = assets.and_then(|assets| assets.background.as_ref()) {
            painted += draw_big_image_at(
                frame,
                background,
                &palette,
                (window.ulx, window.uly),
                window,
            );
        } else {
            frame.fill_rect(window, palette.rgba(0x22));
            painted += 1;
        }

        let font = GnwFontId::Font5.font();

        if self.stage == Stage::Confirm {
            let arm = self.arm_rect();
            let cancel = self.cancel_rect();

            painted += match assets.and_then(|assets| assets.arm.as_ref()) {
                Some(pair) => draw_image_button(
                    frame,
                    &palette,
                    pair.image(false),
                    pair.image(true),
                    arm,
                    "Arm",
                    false,
                ),
                None => {
                    frame.fill_rect(arm, palette.rgba(0x17));
                    painted += 1;
                    draw_text_gnw_centered(frame, arm, font, "Arm", palette.rgba(0xA2)) as u32
                }
            };
            painted += match assets.and_then(|assets| assets.cancel.as_ref()) {
                Some(pair) => draw_image_button(
                    frame,
                    &palette,
                    pair.image(false),
                    pair.image(true),
                    cancel,
                    "Cancel",
                    false,
                ),
                None => {
                    frame.fill_rect(cancel, palette.rgba(0x17));
                    painted += 1;
                    draw_text_gnw_centered(frame, cancel, font, "Cancel", palette.rgba(0xA2)) as u32
                }
            };
        } else {
            let ok = self.ok_rect();

            if self.is_armed() {
                painted += match assets.and_then(|assets| assets.ok.as_ref()) {
                    Some(pair) => {
                        blit_simple(frame, &palette, pair.image(false), ok.ulx, ok.uly, true)
                    }
                    None => {
                        frame.fill_rect(ok, palette.rgba(0x17));
                        1
                    }
                };
            } else if let Some(image) = assets.and_then(|assets| assets.open(self.frame)) {
                painted += blit_simple(frame, &palette, image, ok.ulx, ok.uly, true);
            }
        }

        painted
    }
}

/// Прямоугольник окна в кадре.
fn rect_at(rect: PixelRect) -> PixelRect {
    PixelRect::new(
        DIALOG_RECT.ulx + rect.ulx,
        DIALOG_RECT.uly + rect.uly,
        rect.width,
        rect.height,
    )
}

/// Ассеты диалога: `SELFDSTR`, `SLFDAR`, `SLFDCN`, `SLFDOPN1..6`, `SLFDOK`.
#[derive(Debug, Clone)]
pub struct SelfDestructAssets {
    /// Фон `SELFDSTR` 174×82.
    pub background: Option<BigImage>,
    /// `Arm` (`SLFDAR_U/D`).
    pub arm: Option<MenuButtonPair>,
    /// `Cancel` (`SLFDCN_U/D`).
    pub cancel: Option<MenuButtonPair>,
    /// Кадры `SLFDOPN1..6`.
    pub open_frames: Vec<SimpleImage>,
    /// `OK` (`SLFDOK_U/D`).
    pub ok: Option<MenuButtonPair>,
}

impl SelfDestructAssets {
    /// Грузит ресурсы; отсутствующие — `None`.
    pub fn load(archive: &max_assets::res::ResArchive) -> Self {
        let simple = |name: &str| -> Option<SimpleImage> {
            let bytes = archive.read(name).ok()?;
            SimpleImage::decode(bytes).ok()
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair::new(simple(up)?, simple(down)?))
        };
        let mut open_frames = Vec::new();

        for index in 1..=OPEN_FRAMES {
            if let Some(image) = simple(&format!("SLFDOPN{index}")) {
                open_frames.push(image);
            }
        }

        Self {
            background: archive
                .read("SELFDSTR")
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok()),
            arm: pair("SLFDAR_U", "SLFDAR_D"),
            cancel: pair("SLFDCN_U", "SLFDCN_D"),
            open_frames,
            ok: pair("SLFDOK_U", "SLFDOK_D"),
        }
    }

    /// Кадр анимации `index` (`SLFDOPN1..6`).
    pub fn open(&self, index: usize) -> Option<&SimpleImage> {
        self.open_frames.get(index.min(OPEN_FRAMES - 1))
    }

    /// Все обязательные ресурсы загрузились.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.arm.is_some() && self.cancel.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_centered_in_the_map_area() {
        assert_eq!(DIALOG_RECT.ulx, 180 + (448 - DIALOG_SIZE.0) / 2);
        assert_eq!(DIALOG_RECT.uly, 18 + (448 - DIALOG_SIZE.1) / 2);
        assert_eq!(DIALOG_RECT.width, DIALOG_SIZE.0);
        assert_eq!(DIALOG_RECT.height, DIALOG_SIZE.1);
    }

    #[test]
    fn arm_plays_six_frames_then_shows_ok() {
        let mut menu = SelfDestructMenu::new(UnitId(3));
        assert_eq!(menu.stage, Stage::Confirm);
        assert_eq!(
            menu.input(InputEvent::MouseClick(
                menu.arm_rect().ulx + 2,
                menu.arm_rect().uly + 2
            )),
            SelfDestructAction::None
        );
        assert_eq!(menu.stage, Stage::Armed);
        assert_eq!(menu.frame, 0);
        assert!(!menu.is_armed());

        for _ in 0..OPEN_FRAMES {
            menu.advance(FRAME_SECONDS);
        }
        assert!(menu.is_armed());
        assert_eq!(menu.frame, OPEN_FRAMES - 1);

        assert_eq!(
            menu.input(InputEvent::MouseClick(
                menu.ok_rect().ulx + 2,
                menu.ok_rect().uly + 2
            )),
            SelfDestructAction::Explode
        );
    }

    #[test]
    fn escape_and_cancel_do_not_explode() {
        let mut menu = SelfDestructMenu::new(UnitId(3));

        assert_eq!(
            menu.input(InputEvent::MouseClick(
                menu.cancel_rect().ulx + 2,
                menu.cancel_rect().uly + 2
            )),
            SelfDestructAction::Cancel
        );

        let mut menu = SelfDestructMenu::new(UnitId(3));
        assert_eq!(
            menu.input(InputEvent::Key(Key::Escape)),
            SelfDestructAction::Cancel
        );

        // Enter на первом окне — Arm (C++ `SetPValue(GNW_KB_KEY_RETURN)`).
        let mut menu = SelfDestructMenu::new(UnitId(3));
        assert_eq!(
            menu.input(InputEvent::Key(Key::Enter)),
            SelfDestructAction::None
        );
        assert_eq!(menu.stage, Stage::Armed);
        // Пока анимация не доиграла, Enter не подтверждает.
        assert_eq!(
            menu.input(InputEvent::Key(Key::Enter)),
            SelfDestructAction::None
        );
    }

    #[test]
    fn frames_are_deterministic_at_fixed_step() {
        let mut first = SelfDestructMenu::new(UnitId(1));
        let mut second = SelfDestructMenu::new(UnitId(1));
        first.arm();
        second.arm();

        for _ in 0..10 {
            first.advance(1.0 / 60.0);
            second.advance(1.0 / 60.0);
        }

        assert_eq!(first.frame, second.frame);
        assert!(first.frame > 0);
    }
}
