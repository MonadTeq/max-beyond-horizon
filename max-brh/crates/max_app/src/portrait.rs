//! Unit portrait in `WINDOW_CORNER_FLIC` — an FLC animation (`GameManager_Flic`).
//!
//! Oracle (`max-cpp-port/src/game_manager.cpp`):
//!
//! * `GameManager_MenuUnitSelect` → `GameManager_MenuCreateFlic(unit_type, …)`:
//!   the `WINDOW_CORNER_FLIC` 128×128 window, the animation is taken from
//!   `Unit::GetFlicsAnimation()` (tag `F_*`), the file is `*.FLC` next to `MAX.RES`;
//!   `flicsmgr_construct(…, animate = 2, load_flic_palette = 0)` — a looping
//!   animation, the window palette (not the FLC palette);
//! * `GameManager_AdvanceFlic`: the frame changes once per `TIMER_FPS_TO_MS(15)` —
//!   **15 frames per second** (the `speed` field of the FLC header is not used);
//! * `GameManager_MenuUnitSelect(nullptr)` → `GameManager_MenuDeleteFlic` +
//!   `FillOrRestoreWindow(WINDOW_CORNER_FLIC, COLOR_BLACK)` — black background;
//! * if there is no FLC (`INVALID_ID` for 22 types), `flicsmgr_construct` draws
//!   nothing; the Rust client in this case shows a static sprite ×2
//!   (see `panels::draw_portrait`).
//!
//! # Determinism
//!
//! The frame is derived from accumulated time: `floor(elapsed * 15) % frames`.
//! In headless/QA frames are drawn with `dt = 0` ([`crate::App::local_update`]),
//! so `elapsed` does not grow and the portrait stays on frame 0 — golden frames
//! are stable. In the GUI `dt = 1/60` (`main.rs`), the animation runs at 15 FPS.
//!
//! Frames are decoded once on selection ([`max_assets::flic::Flic::decode_all`]),
//! so rendering does not depend on the playback history and cannot "drift"
//! from the reference.

use max_assets::flic::{Flic, FlicError, FlicFrame};
use max_core::enums::UnitType;
use max_core::unit::UnitId;

use crate::App;

/// Portrait frame rate (`GameManager_AdvanceFlic`,
/// `TIMER_FPS_TO_MS(15)`).
pub const PORTRAIT_FPS: u32 = 15;

/// Animation of the selected unit's portrait: all FLC frames + the animation phase.
#[derive(Debug, Clone)]
pub struct PortraitAnim {
    unit: UnitId,
    unit_type: UnitType,
    frames: Vec<FlicFrame>,
    elapsed: f64,
}

impl PortraitAnim {
    /// Decodes all FLC frames (like `flicsmgr_construct` + `AdvanceFlic`).
    ///
    /// # Errors
    ///
    /// [`FlicError`] — corrupted frames; the caller falls back to the sprite.
    pub fn load(unit: UnitId, unit_type: UnitType, flic: &Flic) -> Result<Self, FlicError> {
        let frames = flic.decode_all()?;

        Ok(Self {
            unit,
            unit_type,
            frames,
            elapsed: 0.0,
        })
    }

    /// Selected unit the animation belongs to.
    #[inline]
    pub fn unit(&self) -> UnitId {
        self.unit
    }

    /// Unit type (owner of the `F_*` resource).
    #[inline]
    pub fn unit_type(&self) -> UnitType {
        self.unit_type
    }

    /// All decoded frames.
    #[inline]
    pub fn frames(&self) -> &[FlicFrame] {
        &self.frames
    }

    /// Number of frames (1 — static FLC).
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Whether the FLC is animated (`frames > 1`, like `frame_count > 1` in C++).
    #[inline]
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Accumulated animation time, seconds.
    #[inline]
    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    /// Frame number: `floor(elapsed * 15)`, wrapped.
    ///
    /// With `elapsed == 0` (headless/QA, `dt = 0`) always 0.
    pub fn frame_index(&self) -> usize {
        if self.frames.len() <= 1 {
            return 0;
        }

        let step = f64::from(self.elapsed.max(0.0)) * f64::from(PORTRAIT_FPS);
        let index = step as usize;

        index % self.frames.len()
    }

    /// Current frame.
    pub fn frame(&self) -> &FlicFrame {
        let index = self.frame_index().min(self.frames.len().saturating_sub(1));

        &self.frames[index]
    }

    /// Advances the animation by `dt` seconds.
    ///
    /// `dt <= 0` (headless/QA) — stays in place: golden frames are deterministic.
    /// Negative and NaN values are ignored (like `dt.max(0.0)` in
    /// `update_visual_units`).
    pub fn advance(&mut self, dt: f32) {
        if !self.is_animated() || !(dt > 0.0) {
            return;
        }

        self.elapsed += f64::from(dt);
    }

    /// Resets the phase (for tests/golden: "the portrait was just selected").
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Integration with App
// ---------------------------------------------------------------------------

impl App {
    /// Current FLC portrait animation (`None` — no selection/FLC).
    #[inline]
    pub fn portrait(&self) -> Option<&PortraitAnim> {
        self.portrait.as_ref()
    }

    /// Synchronizes the animation with the selection: when the unit changes it decodes its FLC
    /// (frame 0); when the selection is cleared it frees it.
    ///
    /// Called from [`App::update_portrait`] and [`App::select_unit`], so
    /// the portrait always corresponds to the selected unit (including in QA frames,
    /// which are drawn without a game frame).
    pub fn refresh_portrait(&mut self) {
        let selected = self.selected_unit().map(|unit| (unit.id, unit.unit_type));

        match selected {
            None => self.portrait = None,
            Some((id, unit_type)) => {
                if self
                    .portrait
                    .as_ref()
                    .is_some_and(|portrait| portrait.unit() == id)
                {
                    return;
                }

                let animation = self
                    .assets
                    .as_ref()
                    .and_then(|assets| assets.flic(unit_type))
                    .and_then(|flic| PortraitAnim::load(id, unit_type, flic).ok());

                self.portrait = animation;
            }
        }
    }

    /// One step of the portrait animation: synchronization with the selection + `dt` seconds.
    ///
    /// `dt = 0` (headless/QA) does not advance the frame — the portrait stays on frame 0.
    pub fn update_portrait(&mut self, dt: f32) {
        self.refresh_portrait();

        if let Some(portrait) = self.portrait.as_mut() {
            portrait.advance(dt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use max_core::enums::UnitType;

    fn frame(value: u8) -> FlicFrame {
        FlicFrame {
            width: 2,
            height: 1,
            pixels: vec![value; 2],
            palette: [[0; 3]; 256],
        }
    }

    fn animation(frames: usize) -> PortraitAnim {
        PortraitAnim {
            unit: UnitId::NO_ID,
            unit_type: UnitType::Tank,
            frames: (0..frames as u8).map(frame).collect(),
            elapsed: 0.0,
        }
    }

    #[test]
    fn dt_zero_keeps_the_portrait_on_frame_zero() {
        let mut portrait = animation(30);

        // 600 frames with dt = 0 — like `local_update(0.0)` in QA.
        for _ in 0..600 {
            portrait.advance(0.0);
        }

        assert_eq!(portrait.elapsed(), 0.0);
        assert_eq!(portrait.frame_index(), 0);
        assert_eq!(portrait.frame().pixels, vec![0, 0]);
    }

    #[test]
    fn fifteen_fps_advances_one_frame_per_sixty_seventh_of_a_second() {
        let mut portrait = animation(3);

        // 1/15 s — exactly the next frame (within float error of dt).
        portrait.advance(1.0 / 15.0);
        assert_eq!(portrait.frame_index(), 1);

        portrait.advance(1.0 / 15.0);
        assert_eq!(portrait.frame_index(), 2);

        // Wrapping: after the 3rd frame — 0 again.
        portrait.advance(1.0 / 15.0);
        assert_eq!(portrait.frame_index(), 0);

        // 60 frames of 1/60 s = one second = 15 animation frames (modulo 3).
        let mut portrait = animation(30);

        for _ in 0..60 {
            portrait.advance(1.0 / 60.0);
        }

        assert!(
            (portrait.elapsed() - 1.0).abs() < 1e-6,
            "{}",
            portrait.elapsed()
        );
        assert_eq!(portrait.frame_index(), 15);
    }

    #[test]
    fn static_flic_never_advances() {
        let mut portrait = animation(1);

        portrait.advance(10.0);

        assert!(!portrait.is_animated());
        assert_eq!(portrait.elapsed(), 0.0);
        assert_eq!(portrait.frame_index(), 0);
    }

    #[test]
    fn reset_returns_to_the_first_frame() {
        let mut portrait = animation(4);

        portrait.advance(0.5);
        assert_ne!(portrait.frame_index(), 0);

        portrait.reset();
        assert_eq!(portrait.frame_index(), 0);
    }
}
