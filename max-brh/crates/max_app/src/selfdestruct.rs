//! Building self-destruct — a port of `UnitsManager_SelfDestructMenu`
//! (`units_manager.cpp:292-357`) and `UnitsManager_SelfDestructActiveMenu`
//! (`:233-290`).
//!
//! # Oracle
//!
//! The `SELFDSTR` window (174×82) in the center of the map area
//! (`Window(SELFDSTR, GameManager_GetDialogWindowCenterMode())` →
//! `(180 + (448-174)/2, 18 + (448-82)/2) = (317, 201)`).
//!
//! * `Arm` — `Button(SLFDAR_U, SLFDAR_D, 89, 14)`, label `_(d5cb)` = "Arm",
//!   sfx `MBUTT0`;
//! * `Cancel` — `Button(SLFDCN_U, SLFDCN_D, 89, 46)`, label `_(ef77)` =
//!   "Cancel", sfx `NCANC0`, `Esc`;
//! * `Enter` presses `Arm` (`SetPValue(GNW_KB_KEY_RETURN)` /
//!   `SetRValue`), after which C++ plays frames `SLFDOPN1..6` every
//!   `TIMER_FPS_TO_MS(48)` and shows the `SLFDOK_U/D` button (61×58 at (13,11),
//!   `SetRValue(GNW_KB_KEY_RETURN)`, sfx `NDONE0`);
//! * confirmation (`Enter`/click on `OK`) returns `true`, cancel — `false`;
//!   `Esc` in any window — cancel (`NCANC0`).
//!
//! # What is ported
//!
//! Both windows and their input; the `SLFDOPN1..6` animation runs on the visual
//! clock (`App::update_visual_units`, 1/60 s step), not on `SDL_Delay`, so the
//! frames are deterministic (golden/QA). The order is `ORDER_EXPLODE`/`ORDER_STATE_EXPLODE`
//! (`UnitsManager_Popup_OnClick_Remove`, `units_manager.cpp:506`).
//!
//! # Deliberate differences
//!
//! * C++ blocks the thread for 6×48 ms inside `SelfDestructActiveMenu`; the port
//!   keeps the window open and accepts `Esc`/`OK` after the animation (as the player
//!   expects), instead of "swallowing" the input.
//! * The second window (`GNW_INPUT_PRESS`) in C++ only highlights the button; the
//!   release sound is not modeled here.

use max_assets::image::{BigImage, SimpleImage};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered, GnwFontId};

use crate::menu::{blit_simple, draw_big_image_at, draw_image_button, MenuButtonPair};
use crate::panels::UiPalette;
use crate::ui::{InputEvent, Key};
use max_core::unit::UnitId;

/// `SELFDSTR` window: 174×82 (`MAX.RES`).
pub const DIALOG_SIZE: (i32, i32) = (174, 82);
/// The window in the frame — center of `WINDOW_MAIN_MAP` (180,18,448,448).
pub const DIALOG_RECT: PixelRect = PixelRect::new(317, 201, 174, 82);
/// `Arm` (`SLFDAR_U/D`, 74×23) at (89,14).
pub const ARM_RECT: PixelRect = PixelRect::new(89, 14, 74, 23);
/// `Cancel` (`SLFDCN_U/D`, 74×23) at (89,46).
pub const CANCEL_RECT: PixelRect = PixelRect::new(89, 46, 74, 23);
/// `OK` (`SLFDOK_U/D`, 61×58) at (13,11).
pub const OK_RECT: PixelRect = PixelRect::new(13, 11, 61, 58);
/// Number of frames `SLFDOPN1..6` (`units_manager.cpp:239`).
pub const OPEN_FRAMES: usize = 6;
/// `TIMER_FPS_TO_MS(48)` — frame step.
pub const FRAME_SECONDS: f32 = 0.048;

/// Dialog stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// First window: `Arm`/`Cancel`.
    Confirm,
    /// Second window: `SLFDOPN` animation and `OK`.
    Armed,
}

/// What to do with input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfDestructAction {
    /// The window stays open.
    None,
    /// Cancel: no order.
    Cancel,
    /// Confirmation: `ORDER_EXPLODE`.
    Explode,
}

/// Open self-destruct dialog.
#[derive(Debug, Clone, PartialEq)]
pub struct SelfDestructMenu {
    /// Building being destroyed.
    pub unit: UnitId,
    /// Stage.
    pub stage: Stage,
    /// Animation frame number `SLFDOPN1..6` (0..=6; 6 — the `OK` button).
    pub frame: usize,
    /// Accumulated frame time.
    elapsed: f32,
}

impl SelfDestructMenu {
    /// Opens the first window (`UnitsManager_SelfDestructMenu`).
    pub fn new(unit: UnitId) -> Self {
        Self {
            unit,
            stage: Stage::Confirm,
            frame: 0,
            elapsed: 0.0,
        }
    }

    /// Image name of the animation frame (`SLFDOPN1..6`).
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

    /// The animation has finished — `OK` is shown.
    pub fn is_armed(&self) -> bool {
        self.stage == Stage::Armed && self.frame + 1 >= OPEN_FRAMES
    }

    /// Button rectangles in the frame.
    pub fn arm_rect(&self) -> PixelRect {
        rect_at(ARM_RECT)
    }

    /// `Cancel`.
    pub fn cancel_rect(&self) -> PixelRect {
        rect_at(CANCEL_RECT)
    }

    /// `OK` of the second window.
    pub fn ok_rect(&self) -> PixelRect {
        rect_at(OK_RECT)
    }

    /// Visual clock (`TIMER_FPS_TO_MS(48)` per frame).
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

    /// Press of `Arm`/`Enter`.
    pub fn arm(&mut self) {
        self.stage = Stage::Armed;
        self.frame = 0;
        self.elapsed = 0.0;
    }

    /// Input: button click or key.
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
                    // The `SLFDOPN1..6` animation is still playing — input is ignored
                    // (C++ runs the frame loop until `OK` appears).
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

    /// Draws the window (`Window(SELFDSTR)` + buttons).
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

/// Window rectangle in the frame.
fn rect_at(rect: PixelRect) -> PixelRect {
    PixelRect::new(
        DIALOG_RECT.ulx + rect.ulx,
        DIALOG_RECT.uly + rect.uly,
        rect.width,
        rect.height,
    )
}

/// Dialog assets: `SELFDSTR`, `SLFDAR`, `SLFDCN`, `SLFDOPN1..6`, `SLFDOK`.
#[derive(Debug, Clone)]
pub struct SelfDestructAssets {
    /// `SELFDSTR` background 174×82.
    pub background: Option<BigImage>,
    /// `Arm` (`SLFDAR_U/D`).
    pub arm: Option<MenuButtonPair>,
    /// `Cancel` (`SLFDCN_U/D`).
    pub cancel: Option<MenuButtonPair>,
    /// Frames `SLFDOPN1..6`.
    pub open_frames: Vec<SimpleImage>,
    /// `OK` (`SLFDOK_U/D`).
    pub ok: Option<MenuButtonPair>,
}

impl SelfDestructAssets {
    /// Loads the resources; missing ones are `None`.
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

    /// Animation frame `index` (`SLFDOPN1..6`).
    pub fn open(&self, index: usize) -> Option<&SimpleImage> {
        self.open_frames.get(index.min(OPEN_FRAMES - 1))
    }

    /// All mandatory resources were loaded.
    pub fn is_ready(&self) -> bool {
        self.background.is_some() && self.arm.is_some() && self.cancel.is_some()
    }
}

#[cfg(test)]
#[path = "selfdestruct_tests.rs"]
mod tests;
