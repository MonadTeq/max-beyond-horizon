//! Отрисовка спрайтов в RGBA-буфер: палитра, командные цвета, тени.
//!
//! Порядок индексации повторяет `Gfx_DecodeSprite` (`gfx.cpp:212`, §5.2 spw.md):
//! **сначала LUT команды**, потом LUT яркости:
//! `цвет = палитра[ brightness[ level ][ team_lut[index] ] ]`.
//!
//! Тень — не пиксели, а маска затемнения (§4.3): [`SpriteRenderer::draw_shadow`]
//! применяет к уже нарисованным пикселям формулу таблицы яркости уровня 5
//! (`c' = c * 5*32 / 224`). Прозрачные пиксели спрайта не пишутся вовсе, так
//! что порядок «тень -> спрайт» даёт картинку оригинала
//! (`UnitInfo::RenderWithConnectors`).
//!
//! Всё это чистые функции над [`RgbaBuffer`]: ни окна, ни SDL, ни состояния.

use max_assets::sprite::{ShadowFrame, SpriteFrame};

use crate::buffer::{PixelRect, Rgba, RgbaBuffer};
use crate::palette::{
    BrightnessLut, ColorLut, Palette, FULL_BRIGHTNESS_LEVEL, SHADOW_BRIGHTNESS_LEVEL,
};

/// Результат одной операции отрисовки.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DrawStats {
    /// Сколько пикселей реально изменено.
    pub drawn: u32,
    /// Сколько пикселей не попало в буфер (клиппинг по границам).
    pub clipped: u32,
}

impl DrawStats {
    /// Сумма результатов (например, тень + спрайт).
    pub fn merge(self, other: Self) -> Self {
        Self {
            drawn: self.drawn + other.drawn,
            clipped: self.clipped + other.clipped,
        }
    }
}

/// Отрисовщик спрайтов: палитра + таблицы перекраски + команда.
///
/// Один и тот же рендерер рисует все кадры одного юнита: команда и уровень
/// яркости — свойства юнита, а не кадра.
#[derive(Debug, Clone, Copy)]
pub struct SpriteRenderer<'a> {
    palette: &'a Palette,
    brightness: &'a BrightnessLut,
    team: ColorLut,
    level: u8,
}

impl<'a> SpriteRenderer<'a> {
    /// Рендерер для команды `team` и уровня яркости `level` (0..=7).
    pub fn new(
        palette: &'a Palette,
        brightness: &'a BrightnessLut,
        team: ColorLut,
        level: u8,
    ) -> Self {
        Self {
            palette,
            brightness,
            team,
            level: level.min(FULL_BRIGHTNESS_LEVEL),
        }
    }

    /// Рендерер нейтральной техники (identity-LUT) на полной яркости.
    pub fn neutral(palette: &'a Palette, brightness: &'a BrightnessLut) -> Self {
        Self::new(
            palette,
            brightness,
            ColorLut::identity(),
            FULL_BRIGHTNESS_LEVEL,
        )
    }

    /// Цвет палитры для индекса спрайта после командной LUT и LUT яркости.
    #[inline]
    pub fn color_for(&self, index: u8) -> Rgba {
        let team_index = self.team.map(index);
        let level_index = self.brightness.map(self.level, team_index);

        self.palette.rgba(level_index)
    }

    /// Уровень яркости рендерера.
    #[inline]
    pub fn level(&self) -> u8 {
        self.level
    }

    /// Командная LUT рендерера.
    #[inline]
    pub fn team(&self) -> &ColorLut {
        &self.team
    }

    /// Рисует кадр так, что горячая точка попадает в `anchor`.
    ///
    /// Позиция — `anchor - hot` (§5.1 при `divisor = 2`, масштаб 1:1).
    /// Прозрачные пиксели не пишутся. За границами буфера — клиппинг.
    pub fn draw_frame(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        anchor: (i32, i32),
    ) -> DrawStats {
        let (ulx, uly) = self.frame_origin(frame, anchor);

        self.draw_rect(dst, frame, (ulx, uly), |_, index| self.color_for(index))
    }

    /// Рисует кадр сплошным силуэтом командного цвета — режим `zoom_level < 8`
    /// (`Gfx_TeamColorIndexBase != 0`, §5.2).
    pub fn draw_frame_silhouette(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        anchor: (i32, i32),
        color: Rgba,
    ) -> DrawStats {
        let (ulx, uly) = self.frame_origin(frame, anchor);

        self.draw_rect(dst, frame, (ulx, uly), |_, _| color)
    }

    /// Затемняет уже нарисованную карту по маске тени.
    ///
    /// Прозрачные пиксели кадра тени пропускаются; уровень затемнения по
    /// умолчанию — [`SHADOW_BRIGHTNESS_LEVEL`] (5). Затемняются только
    /// существующие пиксели буфера.
    pub fn draw_shadow(
        &self,
        dst: &mut RgbaBuffer,
        frame: &ShadowFrame,
        anchor: (i32, i32),
    ) -> DrawStats {
        self.draw_shadow_at_level(dst, frame, anchor, SHADOW_BRIGHTNESS_LEVEL)
    }

    /// То же, но с явным уровнем затемнения (0..=7).
    pub fn draw_shadow_at_level(
        &self,
        dst: &mut RgbaBuffer,
        frame: &ShadowFrame,
        anchor: (i32, i32),
        level: u8,
    ) -> DrawStats {
        let ulx = anchor.0 - i32::from(frame.hotx);
        let uly = anchor.1 - i32::from(frame.hoty);
        let mut stats = DrawStats::default();

        for y in 0..frame.height as i32 {
            for x in 0..frame.width as i32 {
                if !frame.darkened(x as u32, y as u32) {
                    continue;
                }

                if dst.modify(ulx + x, uly + y, |color| color.darken(level)) {
                    stats.drawn += 1;
                } else {
                    stats.clipped += 1;
                }
            }
        }

        stats
    }

    /// Рисует юнит в порядке оригинала: тень (со смещением `shadow_offset`),
    /// затем спрайт (`UnitInfo::RenderWithConnectors`, §5.4).
    pub fn draw_unit(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        shadow: Option<&ShadowFrame>,
        anchor: (i32, i32),
        shadow_offset: (i32, i32),
    ) -> DrawStats {
        let mut stats = DrawStats::default();

        if let Some(shadow) = shadow {
            stats = stats.merge(self.draw_shadow(
                dst,
                shadow,
                (anchor.0 + shadow_offset.0, anchor.1 + shadow_offset.1),
            ));
        }

        stats.merge(self.draw_frame(dst, frame, anchor))
    }

    /// Буфер `width × height` с одним кадром (якорь в горячей точке).
    ///
    /// Удобно для тестов и для кэширования кадра как текстуры.
    pub fn render_frame(&self, frame: &SpriteFrame) -> RgbaBuffer {
        let mut buffer = RgbaBuffer::new(u32::from(frame.width), u32::from(frame.height));

        self.draw_frame(
            &mut buffer,
            frame,
            (i32::from(frame.hotx), i32::from(frame.hoty)),
        );

        buffer
    }

    /// Прямоугольник, который займёт кадр в координатах буфера (до клиппинга).
    pub fn frame_rect(&self, frame: &SpriteFrame, anchor: (i32, i32)) -> PixelRect {
        let (ulx, uly) = self.frame_origin(frame, anchor);

        PixelRect::new(ulx, uly, i32::from(frame.width), i32::from(frame.height))
    }

    fn frame_origin(&self, frame: &SpriteFrame, anchor: (i32, i32)) -> (i32, i32) {
        (
            anchor.0 - i32::from(frame.hotx),
            anchor.1 - i32::from(frame.hoty),
        )
    }

    fn draw_rect(
        &self,
        dst: &mut RgbaBuffer,
        frame: &SpriteFrame,
        origin: (i32, i32),
        color: impl Fn(u32, u8) -> Rgba,
    ) -> DrawStats {
        let mut stats = DrawStats::default();
        let width = frame.width as i32;
        let height = frame.height as i32;

        for y in 0..height {
            for x in 0..width {
                let Some(index) = frame.pixel(x as u32, y as u32) else {
                    continue;
                };

                if dst.set(origin.0 + x, origin.1 + y, color(y as u32, index)) {
                    stats.drawn += 1;
                } else {
                    stats.clipped += 1;
                }
            }
        }

        stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{Palette, PALETTE_BYTES};
    use max_assets::sprite::{decode_shadow, decode_sprite};

    /// Палитра, у которой 8-битный цвет = `((index % 64) * 4, ...)`.
    fn ramp_palette() -> Palette {
        let mut bytes = vec![0u8; PALETTE_BYTES];

        for index in 0..256usize {
            let value = (index % 64) as u8;

            bytes[index * 3] = value;
            bytes[index * 3 + 1] = value;
            bytes[index * 3 + 2] = value;
        }

        Palette::from_color_pal(&bytes).unwrap()
    }

    /// Собирает тело из одного кадра; `rows` — готовые закодированные строки.
    fn build_sprite(width: u16, height: u16, hotx: i16, hoty: i16, rows: Vec<Vec<u8>>) -> Vec<u8> {
        assert_eq!(rows.len(), height as usize);

        let mut out = Vec::new();

        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&6u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&hotx.to_le_bytes());
        out.extend_from_slice(&hoty.to_le_bytes());

        let table = out.len();
        out.extend(std::iter::repeat_n(0u8, 4 * height as usize));

        for (y, row) in rows.iter().enumerate() {
            let offset = out.len() as u32;
            out[table + 4 * y..table + 4 * y + 4].copy_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(row);
        }

        out
    }

    /// Спрайт 3×2: строка 0 — два пикселя `1, 2` со skip 1; строка 1 — пиксель `3`.
    fn sample_sprite() -> Vec<u8> {
        build_sprite(
            3,
            2,
            1,
            1,
            vec![vec![1, 2, 1, 2, 0xFF], vec![0, 1, 3, 0xFF]],
        )
    }

    fn renderer<'a>(palette: &'a Palette, brightness: &'a BrightnessLut) -> SpriteRenderer<'a> {
        SpriteRenderer::neutral(palette, brightness)
    }

    #[test]
    fn frame_pixels_are_palette_colors_and_anchor_uses_hot_spot() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();
        let renderer = renderer(&palette, &brightness);

        let buffer = renderer.render_frame(frame);

        assert_eq!(buffer.width(), 3);
        assert_eq!(buffer.height(), 2);
        // Строка 0: (0,0) прозрачен, (1,0) = 1, (2,0) = 2.
        assert_eq!(buffer.get(0, 0), Some(Rgba::TRANSPARENT));
        assert_eq!(buffer.get(1, 0), Some(palette.rgba(1)));
        assert_eq!(buffer.get(2, 0), Some(palette.rgba(2)));
        // Строка 1: (0,1) = 3, остальные прозрачны.
        assert_eq!(buffer.get(0, 1), Some(palette.rgba(3)));
        assert_eq!(buffer.get(1, 1), Some(Rgba::TRANSPARENT));

        // В буфере 8×8 якорь (4,4) при hot (1,1) -> левый верхний угол (3,3).
        let mut target = RgbaBuffer::new(8, 8);
        let stats = renderer.draw_frame(&mut target, frame, (4, 4));

        assert_eq!(
            stats,
            DrawStats {
                drawn: 3,
                clipped: 0
            }
        );
        assert_eq!(target.get(3, 3), Some(Rgba::TRANSPARENT));
        assert_eq!(target.get(4, 3), Some(palette.rgba(1)));
        assert_eq!(target.get(5, 3), Some(palette.rgba(2)));
        assert_eq!(target.get(3, 4), Some(palette.rgba(3)));
        assert_eq!(
            renderer.frame_rect(frame, (4, 4)),
            PixelRect::new(3, 3, 3, 2)
        );
    }

    #[test]
    fn command_slots_are_recoloured_by_team_lut() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        // Индекс 34 — командный слот 32–39.
        let body = build_sprite(2, 1, 0, 0, vec![vec![0, 2, 34, 35, 0xFF]]);
        let sheet = decode_sprite(&body).unwrap();
        let frame = sheet.frame(0).unwrap();

        let red = SpriteRenderer::new(
            &palette,
            &brightness,
            ColorLut::for_team(crate::palette::TeamColors::Red),
            FULL_BRIGHTNESS_LEVEL,
        );
        let green = SpriteRenderer::new(
            &palette,
            &brightness,
            ColorLut::for_team(crate::palette::TeamColors::Green),
            FULL_BRIGHTNESS_LEVEL,
        );

        // Красная команда: 34 -> 58, 35 -> 59.
        assert_eq!(red.color_for(34), palette.rgba(58));
        assert_eq!(red.color_for(35), palette.rgba(59));
        // Зелёная: слоты 32–39 — родные индексы, перекраски нет.
        assert_eq!(green.color_for(34), palette.rgba(34));
        assert_eq!(green.color_for(35), palette.rgba(35));

        let red_buffer = red.render_frame(frame);
        let green_buffer = green.render_frame(frame);

        // Красная: 34 -> 58, 35 -> 59; зелёная — без перекраски.
        assert_eq!(red_buffer.get(0, 0), Some(palette.rgba(58)));
        assert_eq!(red_buffer.get(1, 0), Some(palette.rgba(59)));
        assert_eq!(green_buffer.get(0, 0), Some(palette.rgba(34)));
        assert_eq!(green_buffer.get(1, 0), Some(palette.rgba(35)));
        assert_ne!(red_buffer.get(1, 0), green_buffer.get(1, 0));
    }

    #[test]
    fn brightness_level_changes_all_opaque_pixels() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();

        let full = SpriteRenderer::neutral(&palette, &brightness);
        let dim = SpriteRenderer::new(
            &palette,
            &brightness,
            ColorLut::identity(),
            SHADOW_BRIGHTNESS_LEVEL,
        );

        let bright_buffer = full.render_frame(frame);
        let dim_buffer = dim.render_frame(frame);

        // Индекс 1 при уровне 5 уходит в более тёмный цвет палитры.
        assert_eq!(bright_buffer.get(1, 0), Some(palette.rgba(1)));
        assert_eq!(
            dim_buffer.get(1, 0),
            Some(palette.rgba(brightness.map(5, 1)))
        );
        assert_ne!(bright_buffer.get(1, 0), dim_buffer.get(1, 0));
        // Полная яркость — identity.
        assert_eq!(brightness.map(FULL_BRIGHTNESS_LEVEL, 1), 1);
    }

    #[test]
    fn shadow_darkens_existing_pixels_only() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let renderer = renderer(&palette, &brightness);

        // Тень 4×1: {1, 2} -> затемняются x = 1, 2.
        let body = build_sprite(4, 1, 0, 0, vec![vec![1, 2, 0xFF]]);
        let shadow = decode_shadow(&body).unwrap();
        let frame = shadow.frame(0).unwrap();

        let base = Rgba::rgb(210, 140, 70);
        let mut target = RgbaBuffer::filled(4, 2, base);
        let stats = renderer.draw_shadow(&mut target, frame, (0, 0));

        assert_eq!(
            stats,
            DrawStats {
                drawn: 2,
                clipped: 0
            }
        );
        // Уровень 5: 210 -> 150, 140 -> 100, 70 -> 50.
        assert_eq!(target.get(0, 0), Some(base));
        assert_eq!(target.get(1, 0), Some(base.darken(5)));
        assert_eq!(target.get(2, 0), Some(base.darken(5)));
        assert_eq!(target.get(3, 0), Some(base));
        // Вторая строка не затронута: у тени height = 1.
        assert_eq!(target.get(1, 1), Some(base));
        assert_eq!(base.darken(5), Rgba::rgb(150, 100, 50));
    }

    #[test]
    fn draw_unit_paints_shadow_before_sprite() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let renderer = renderer(&palette, &brightness);

        let sprite = decode_sprite(&build_sprite(2, 1, 0, 0, vec![vec![0, 1, 9, 0xFF]])).unwrap();
        let shadow = decode_shadow(&build_sprite(2, 1, 0, 0, vec![vec![0, 2, 0xFF]])).unwrap();

        let mut target = RgbaBuffer::filled(2, 1, Rgba::rgb(255, 255, 255));
        let stats = renderer.draw_unit(
            &mut target,
            sprite.frame(0).unwrap(),
            Some(shadow.frame(0).unwrap()),
            (0, 0),
            (0, 0),
        );

        assert_eq!(stats.drawn, 3);
        // Пиксель 1: сначала затемнён (тень), потом перекрыт спрайтом (индекс 9).
        assert_eq!(target.get(0, 0), Some(palette.rgba(9)));
        assert_eq!(target.get(1, 0), Some(Rgba::rgb(255, 255, 255).darken(5)));
    }

    #[test]
    fn clipping_counts_pixels_outside_the_buffer() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();
        let renderer = renderer(&palette, &brightness);

        let mut target = RgbaBuffer::new(2, 1);
        // Якорь (1,1) при hot (1,1) -> кадр начинается в (0,0), но высота
        // буфера всего 1 пиксель: пиксели (2,0) и (0,1) кадра отсекаются.
        let stats = renderer.draw_frame(&mut target, frame, (1, 1));

        assert_eq!(
            stats,
            DrawStats {
                drawn: 1,
                clipped: 2
            }
        );
        assert_eq!(target.get(1, 0), Some(palette.rgba(1)));
        assert_eq!(target.get(0, 0), Some(Rgba::TRANSPARENT));
    }

    #[test]
    fn silhouette_mode_ignores_palette() {
        let palette = ramp_palette();
        let brightness = BrightnessLut::build(&palette);
        let sheet = decode_sprite(&sample_sprite()).unwrap();
        let frame = sheet.frame(0).unwrap();
        let renderer = renderer(&palette, &brightness);

        let color = Rgba::rgb(200, 30, 40);
        let mut target = RgbaBuffer::new(8, 8);
        let stats = renderer.draw_frame_silhouette(&mut target, frame, (4, 4), color);

        assert_eq!(stats.drawn, 3);
        assert_eq!(target.get(4, 3), Some(color));
        assert_eq!(target.get(5, 3), Some(color));
        assert_eq!(target.get(3, 4), Some(color));
        assert_eq!(target.get(3, 3), Some(Rgba::TRANSPARENT));
    }
}
