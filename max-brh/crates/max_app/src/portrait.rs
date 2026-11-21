//! Портрет юнита в `WINDOW_CORNER_FLIC` — FLC-анимация (`GameManager_Flic`).
//!
//! Оракул (`max-cpp-port/src/game_manager.cpp`):
//!
//! * `GameManager_MenuUnitSelect` → `GameManager_MenuCreateFlic(unit_type, …)`:
//!   окно `WINDOW_CORNER_FLIC` 128×128, анимация берётся из
//!   `Unit::GetFlicsAnimation()` (тег `F_*`), файл — `*.FLC` рядом с `MAX.RES`;
//!   `flicsmgr_construct(…, animate = 2, load_flic_palette = 0)` — зацикленная
//!   анимация, палитра окна (не палитра FLC);
//! * `GameManager_AdvanceFlic`: кадр меняется раз в `TIMER_FPS_TO_MS(15)` —
//!   **15 кадров в секунду** (поле `speed` заголовка FLC не используется);
//! * `GameManager_MenuUnitSelect(nullptr)` → `GameManager_MenuDeleteFlic` +
//!   `FillOrRestoreWindow(WINDOW_CORNER_FLIC, COLOR_BLACK)` — чёрный фон;
//! * если FLC нет (`INVALID_ID` у 22 типов) — `flicsmgr_construct` не рисует
//!   ничего; Rust-клиент в этом случае показывает статичный спрайт ×2
//!   (см. `panels::draw_portrait`).
//!
//! # Детерминизм
//!
//! Кадр выводится из накопленного времени: `floor(elapsed * 15) % frames`.
//! В headless/QA кадры рисуются с `dt = 0` ([`crate::App::local_update`]),
//! поэтому `elapsed` не растёт и портрет стоит на кадре 0 — golden-кадры
//! стабильны. В GUI `dt = 1/60` (`main.rs`), анимация идёт с 15 FPS.
//!
//! Кадры декодируются один раз при выделении ([`max_assets::flic::Flic::decode_all`]),
//! поэтому отрисовка не зависит от истории проигрывания и не может «разъехаться»
//! с эталоном.

use max_assets::flic::{Flic, FlicError, FlicFrame};
use max_core::enums::UnitType;
use max_core::unit::UnitId;

use crate::App;

/// Частота смены кадров портрета (`GameManager_AdvanceFlic`,
/// `TIMER_FPS_TO_MS(15)`).
pub const PORTRAIT_FPS: u32 = 15;

/// Анимация портрета выделенного юнита: все кадры FLC + фаза анимации.
#[derive(Debug, Clone)]
pub struct PortraitAnim {
    unit: UnitId,
    unit_type: UnitType,
    frames: Vec<FlicFrame>,
    elapsed: f64,
}

impl PortraitAnim {
    /// Декодирует все кадры FLC (как `flicsmgr_construct` + `AdvanceFlic`).
    ///
    /// # Ошибки
    ///
    /// [`FlicError`] — битые кадры; вызывающий откатывается на спрайт.
    pub fn load(unit: UnitId, unit_type: UnitType, flic: &Flic) -> Result<Self, FlicError> {
        let frames = flic.decode_all()?;

        Ok(Self {
            unit,
            unit_type,
            frames,
            elapsed: 0.0,
        })
    }

    /// Выделенный юнит, к которому относится анимация.
    #[inline]
    pub fn unit(&self) -> UnitId {
        self.unit
    }

    /// Тип юнита (владелец `F_*`-ресурса).
    #[inline]
    pub fn unit_type(&self) -> UnitType {
        self.unit_type
    }

    /// Все декодированные кадры.
    #[inline]
    pub fn frames(&self) -> &[FlicFrame] {
        &self.frames
    }

    /// Число кадров (1 — статичный FLC).
    #[inline]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Анимируется ли FLC (`frames > 1`, как `frame_count > 1` в C++).
    #[inline]
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Накопленное время анимации, секунды.
    #[inline]
    pub fn elapsed(&self) -> f64 {
        self.elapsed
    }

    /// Номер кадра: `floor(elapsed * 15)`, зациклено.
    ///
    /// При `elapsed == 0` (headless/QA, `dt = 0`) всегда 0.
    pub fn frame_index(&self) -> usize {
        if self.frames.len() <= 1 {
            return 0;
        }

        let step = f64::from(self.elapsed.max(0.0)) * f64::from(PORTRAIT_FPS);
        let index = step as usize;

        index % self.frames.len()
    }

    /// Текущий кадр.
    pub fn frame(&self) -> &FlicFrame {
        let index = self.frame_index().min(self.frames.len().saturating_sub(1));

        &self.frames[index]
    }

    /// Продвигает анимацию на `dt` секунд.
    ///
    /// `dt <= 0` (headless/QA) — стоит на месте: golden-кадры детерминированы.
    /// Отрицательные и NaN значения игнорируются (как `dt.max(0.0)` в
    /// `update_visual_units`).
    pub fn advance(&mut self, dt: f32) {
        if !self.is_animated() || !(dt > 0.0) {
            return;
        }

        self.elapsed += f64::from(dt);
    }

    /// Сбрасывает фазу (для тестов/golden: «портрет только что выделили»).
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
    }
}

// ---------------------------------------------------------------------------
// Интеграция с App
// ---------------------------------------------------------------------------

impl App {
    /// Текущая FLC-анимация портрета (`None` — нет выделения/FLC).
    #[inline]
    pub fn portrait(&self) -> Option<&PortraitAnim> {
        self.portrait.as_ref()
    }

    /// Синхронизирует анимацию с выделением: при смене юнита декодирует его FLC
    /// (кадр 0), при снятии выделения — освобождает.
    ///
    /// Вызывается из [`App::update_portrait`] и [`App::select_unit`], поэтому
    /// портрет всегда соответствует выделенному юниту (в т.ч. в QA-кадрах,
    /// которые рисуются без игрового кадра).
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

    /// Один шаг анимации портрета: синхронизация с выделением + `dt` секунд.
    ///
    /// `dt = 0` (headless/QA) не двигает кадр — портрет стоит на кадре 0.
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

        // 600 кадров по dt = 0 — как `local_update(0.0)` в QA.
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

        // 1/15 с — ровно следующий кадр (в пределах float-погрешности dt).
        portrait.advance(1.0 / 15.0);
        assert_eq!(portrait.frame_index(), 1);

        portrait.advance(1.0 / 15.0);
        assert_eq!(portrait.frame_index(), 2);

        // Зацикливание: после 3-го кадра — снова 0.
        portrait.advance(1.0 / 15.0);
        assert_eq!(portrait.frame_index(), 0);

        // 60 кадров по 1/60 с = секунда = 15 кадров анимации (модуло 3).
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
