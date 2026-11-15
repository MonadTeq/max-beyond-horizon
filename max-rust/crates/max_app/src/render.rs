//! Кадр без SDL: карта + юниты в [`RgbaBuffer`].
//!
//! Здесь собраны только **чистые** части [`max_render`]:
//! [`render_map_into`] (тайлы/поверхности/ресурсы), [`SpriteRenderer`]
//! (палитра, командные LUT, тени) и [`draw_order`] (порядок юнитов по `y`,
//! §5.4). SDL-слой (`max_render::sdl_window`) подключается исключительно в
//! `src/main.rs` под feature `sdl`, поэтому приложение и его тесты рисуют кадры
//! без окна и без дисплея.
//!
//! Если ассеты не загружены ([`App::init_assets`] не вызывался) или спрайта
//! типа нет в [`crate::assets::UNIT_SPRITES`], юнит рисуется квадратным
//! маркером командного цвета. Карта при этом всегда рисуется настоящими
//! цветами поверхности/ресурсов из [`TileStyle`].

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::map_render::{render_map_into, TileStyle};
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::viewport::draw_order;

use crate::App;

/// Диагностика одного кадра.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Сколько клеток карты нарисовано.
    pub map_tiles: u32,
    /// Сколько юнитов нарисовано спрайтами.
    pub units_drawn: usize,
    /// Сколько юнитов нарисовано маркерами (нет спрайта/ассетов).
    pub units_marker: usize,
    /// Сколько юнитов пропущено (нет в списках/невидимы камере).
    pub units_skipped: usize,
}

impl RenderStats {
    /// Всего нарисованных юнитов.
    pub fn units(self) -> usize {
        self.units_drawn + self.units_marker
    }
}

/// Цвета маркеров команд (запасной вариант без спрайтов): порядок команд
/// оригинала — `RED, GREEN, BLUE, GRAY, ALIEN`.
const MARKER_COLORS: [Rgba; 5] = [
    Rgba::rgb(224, 48, 48),
    Rgba::rgb(48, 192, 72),
    Rgba::rgb(64, 112, 224),
    Rgba::rgb(192, 192, 192),
    Rgba::rgb(212, 168, 64),
];

impl App {
    /// Рисует кадр текущего состояния в буфер `screen_width × screen_height`.
    pub fn render_frame(&self) -> RgbaBuffer {
        self.render_frame_with_stats().0
    }

    /// То же, что [`App::render_frame`], плюс статистика отрисовки.
    ///
    /// Если загружен хром окна (`FRAMEPIC`) и размер кадра — 640×480, рисуется
    /// окно 1:1 как в оригинале: фон-панели из `MAX.RES`, а карта — в области
    /// [`max_render::layout::MAIN_MAP`] (7×7 клеток). Иначе — прежний
    /// полноэкранный режим.
    pub fn render_frame_with_stats(&self) -> (RgbaBuffer, RenderStats) {
        let (width, height) = self.viewport().screen();

        if width == max_render::layout::WINDOW_WIDTH as u32
            && height == max_render::layout::WINDOW_HEIGHT as u32
        {
            if let Some(chrome) = self.assets().and_then(|assets| assets.chrome.as_ref()) {
                if chrome.is_ready() {
                    return self.render_with_chrome(chrome);
                }
            }
        }

        let style = TileStyle::default();
        let mut frame = RgbaBuffer::filled(width, height, style.unknown);
        let mut stats = RenderStats::default();

        // Поверхность: если загружены данные планеты — настоящие тайлы 64×64 из
        // `.WRL` (как в оригинале), иначе процедурная заглушка `TileStyle`.
        match self.assets() {
            Some(assets) if !assets.terrain.is_empty() => {
                stats.map_tiles = assets.terrain.render_into(&mut frame, self.viewport());
                max_render::map_render::render_resources_into(
                    &mut frame,
                    &self.state().map,
                    self.viewport(),
                    &style,
                );
            }
            _ => {
                stats.map_tiles =
                    render_map_into(&mut frame, &self.state().map, self.viewport(), &style);
            }
        }

        self.draw_units(&mut frame, self.viewport(), &mut stats);

        (frame, stats)
    }

    /// Кадр в окне 1:1: `FRAMEPIC` + карта в области карты.
    fn render_with_chrome(&self, chrome: &crate::chrome::Chrome) -> (RgbaBuffer, RenderStats) {
        let style = TileStyle::default();
        let mut frame = RgbaBuffer::filled(
            max_render::layout::WINDOW_WIDTH as u32,
            max_render::layout::WINDOW_HEIGHT as u32,
            style.unknown,
        );
        chrome.draw(&mut frame);

        // Карта рисуется в собственный буфер размером с область карты: так она
        // сама обрезается по краям `MAIN_MAP` (как в оригинале), а камера и зум
        // остаются общими с приложением. `map_viewport` — та же камера, которой
        // пользуется ввод мыши (`App::map_viewport`), иначе клики уезжают.
        let area = max_render::layout::MAIN_MAP;
        let viewport = self.map_viewport();
        let mut map = RgbaBuffer::filled(area.width as u32, area.height as u32, style.unknown);
        let mut stats = RenderStats::default();

        match self.assets() {
            Some(assets) if !assets.terrain.is_empty() => {
                stats.map_tiles = assets.terrain.render_into(&mut map, &viewport);
                max_render::map_render::render_resources_into(
                    &mut map,
                    &self.state().map,
                    &viewport,
                    &style,
                );
            }
            _ => {
                stats.map_tiles = render_map_into(&mut map, &self.state().map, &viewport, &style);
            }
        }

        self.draw_units(&mut map, &viewport, &mut stats);
        crate::chrome::blit_map(&mut frame, &map);

        // Панели поверх карты: кнопки, портрет, статистика, сетка, зум,
        // координаты — как `GameManager_Draw*` в оригинале.
        if let Some(panels) = self.assets().and_then(|assets| assets.panels.as_ref()) {
            panels.draw(&mut frame, self);
        }

        (frame, stats)
    }

    /// Юниты в буфер через камеру: только видимые и известные команде.
    fn draw_units(
        &self,
        frame: &mut RgbaBuffer,
        viewport: &max_render::viewport::Viewport,
        stats: &mut RenderStats,
    ) {
        let mut positions = Vec::new();
        let mut visible = Vec::new();

        for id in self.state().unit_ids_in_order() {
            let Some(unit) = self.state().unit(id) else {
                stats.units_skipped += 1;
                continue;
            };
            if !viewport.is_tile_visible(unit.grid.x, unit.grid.y) {
                continue;
            }
            if !unit.is_visible_to_team(self.state().player_team) {
                continue;
            }
            positions.push((unit.grid.x, unit.grid.y));
            visible.push(id);
        }

        for index in draw_order(&positions) {
            let Some(unit) = self.state().unit(visible[index]) else {
                continue;
            };
            let rect = viewport.tile_rect(unit.grid.x, unit.grid.y);
            let anchor = (rect.ulx + rect.width / 2, rect.uly + rect.height / 2);

            let drawn = self.assets().and_then(|assets| {
                let sprite = assets.sprite(unit.unit_type)?;
                let sprite_frame = sprite.frame_for(unit.image_base, unit.angle)?;
                let shadow = sprite.shadow_for(unit.image_base, unit.angle);
                let colors = TeamColors::ALL
                    .get(unit.team.0 as usize)
                    .copied()
                    .unwrap_or(TeamColors::Derelict);
                let level = unit.brightness.min(FULL_BRIGHTNESS_LEVEL);
                let renderer = SpriteRenderer::new(
                    &assets.palette,
                    &assets.brightness,
                    ColorLut::for_team(colors),
                    level,
                );

                renderer.draw_unit(
                    frame,
                    sprite_frame,
                    shadow,
                    anchor,
                    (
                        i32::from(unit.shadow_offset.x),
                        i32::from(unit.shadow_offset.y),
                    ),
                );

                Some(())
            });

            match drawn {
                Some(()) => stats.units_drawn += 1,
                None => {
                    draw_marker(frame, anchor, unit.team.0);
                    stats.units_marker += 1;
                }
            }
        }

    }
}

/// Маркер юнита 6×6 в точке `anchor` (центр клетки).
fn draw_marker(frame: &mut RgbaBuffer, anchor: (i32, i32), team: u8) {
    let color = MARKER_COLORS
        .get(usize::from(team))
        .copied()
        .unwrap_or(MARKER_COLORS[4]);

    for y in -3..=3 {
        for x in -3..=3 {
            frame.set(anchor.0 + x, anchor.1 + y, color);
        }
    }
}

/// Пишет [`RgbaBuffer`] в бинарный PPM (`P6`) — просмотр кадра без SDL.
///
/// PPM выбран потому, что формат — три строки заголовка и сырой RGB, без
/// кодеков и зависимостей: кадр из headless-прогона можно открыть любым
/// просмотрщиком изображений.
pub fn write_ppm(path: impl AsRef<Path>, buffer: &RgbaBuffer) -> io::Result<()> {
    let path = path.as_ref();
    let file = fs::File::create(path)?;
    let mut writer = io::BufWriter::new(file);

    write!(writer, "P6\n{} {}\n255\n", buffer.width(), buffer.height())?;

    let mut rgb = Vec::with_capacity(buffer.len() * 3);
    for pixel in buffer.pixels().chunks_exact(4) {
        rgb.extend_from_slice(&pixel[..3]);
    }

    writer.write_all(&rgb)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[test]
    fn frame_without_assets_has_screen_size_and_is_opaque() {
        let app = App::new(AppConfig::default().with_screen(64, 48));
        let (frame, stats) = app.render_frame_with_stats();

        assert_eq!(frame.width(), 64);
        assert_eq!(frame.height(), 48);
        assert_eq!(frame.len(), 64 * 48);
        assert!(stats.map_tiles > 0, "карта должна рисоваться без ассетов");
        assert_eq!(stats.units(), stats.units_drawn + stats.units_marker);
        assert!(frame.pixels().chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn ppm_header_is_p6_with_frame_dimensions() {
        let path = std::env::temp_dir().join("max_app_render_test.ppm");
        let frame = RgbaBuffer::filled(3, 2, Rgba::rgb(1, 2, 3));

        write_ppm(&path, &frame).expect("ppm written");
        let bytes = std::fs::read(&path).expect("ppm read back");
        let header = b"P6\n3 2\n255\n";

        assert_eq!(&bytes[..header.len()], header);
        assert_eq!(bytes.len(), header.len() + 3 * 2 * 3);
        assert_eq!(&bytes[header.len()..header.len() + 3], &[1, 2, 3]);

        let _ = std::fs::remove_file(&path);
    }
}
