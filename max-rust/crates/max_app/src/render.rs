//! Кадр без SDL: карта + юниты в [`RgbaBuffer`].
//!
//! Здесь собраны только **чистые** части [`max_render`]:
//! [`render_map_into`] (тайлы/поверхности/ресурсы), [`SpriteRenderer`]
//! (палитра, командные LUT, тени) и [`draw_order`] (порядок юнитов по `y`,
//! §5.4). SDL-слой (`max_render::sdl_window`) подключается исключительно в
//! `src/main.rs` под feature `sdl`, поэтому приложение и его тесты рисуют кадры
//! без окна и без дисплея.
//!
//! Если ассеты не загружены ([`App::init_assets`] не вызывался) или ресурса
//! типа нет в [`crate::assets::UNIT_SPRITES`], юнит рисуется квадратным
//! маркером командного цвета. В данных игры такой тип ровно один — `MASTER`
//! (`UNIT_TYPES_WITHOUT_SPRITE_DATA`); карта при этом всегда рисуется
//! настоящими цветами поверхности/ресурсов из [`TileStyle`].
//!
//! Общая точка отрисовки юнита — [`draw_unit_sprites`]: её используют и карта
//! (`App::draw_units`), и HUD-портрет (`panels::draw_portrait`), поэтому все
//! 93 типа рисуются одинаково (корпус, тень, башня со сдвигом).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use max_render::buffer::{Rgba, RgbaBuffer};
use max_render::map_render::{render_map_into, TileStyle};
use max_render::palette::{ColorLut, TeamColors, FULL_BRIGHTNESS_LEVEL};
use max_render::sprite_atlas::SpriteRenderer;
use max_render::terrain::PaletteRgba;
use max_render::viewport::draw_order;

use max_core::enums::unit_flags;

use crate::panels::UiPalette;
use crate::App;
/// Диагностика одного кадра.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Сколько клеток карты нарисовано.
    pub map_tiles: u32,
    /// Сколько маркеров ресурсов нарисовано.
    pub resource_icons: u32,
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

/// Что нарисовано для одного юнита (диагностика и тесты покрытия типов).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitSpriteStats {
    /// Пиксели корпуса (вместе с тенью — она только затемняет уже
    /// нарисованное, поэтому на пустом кадре это ровно корпус).
    pub unit: u32,
    /// Пиксели башни/пушки; 0 — у типа нет башни или флага.
    pub turret: u32,
}

/// Рисует юнит спрайтами: тень → корпус → башня/пушка.
///
/// Повторяет `UnitInfo::RenderWithConnectors`/`Render` C++: корпус с
/// `zoom/64`, тень со сдвигом `shadow_offset`, башня отдельным кадром
/// `turret_image_base + turret_angle` со сдвигом `angle_offsets[angle]`.
///
/// Возвращает `None`, если для типа нет загруженного спрайта/кадра — тогда
/// вызывающий рисует маркер команды (единственный такой тип в данных игры —
/// `MASTER`, см. [`crate::assets::UNIT_TYPES_WITHOUT_SPRITE_DATA`]).
///
/// Это общая точка карты ([`App::draw_units`]), HUD-портрета
/// (`panels::draw_portrait`) и тестов, которые гоняют рендер по всем 93
/// типам юнитов.
pub fn draw_unit_sprites(
    assets: &crate::assets::Assets,
    unit: &max_core::unit::UnitInfo,
    frame: &mut RgbaBuffer,
    anchor: (i32, i32),
    zoom: u32,
) -> Option<UnitSpriteStats> {
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

    let shadow_offset = (
        i32::from(unit.shadow_offset.x),
        i32::from(unit.shadow_offset.y),
    );
    let mut stats = UnitSpriteStats::default();

    stats.unit = renderer
        .draw_unit_scaled(frame, sprite_frame, shadow, anchor, shadow_offset, zoom)
        .drawn;

    // Башня/пушка — отдельный кадр поверх корпуса со сдвигом
    // `angle_offsets[angle]` (`UnitInfo::RenderWithConnectors`).
    if unit.flags & (unit_flags::TURRET_SPRITE | unit_flags::SPINNING_TURRET) != 0 {
        if let Some(turret) = sprite.turret_frame(unit.turret_angle) {
            stats.turret = renderer
                .draw_turret_scaled(
                    frame,
                    turret,
                    sprite.turret_shadow(unit.turret_angle),
                    anchor,
                    shadow_offset,
                    sprite.turret_offset(unit.angle),
                    zoom,
                )
                .drawn;
        }
    }

    Some(stats)
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
                stats.resource_icons = render_resources(
                    &mut frame,
                    assets,
                    &self.state().map,
                    self.viewport(),
                    &style,
                    self.state().player_team.0,
                    self.hud().panels.survey,
                );
            }
            _ => {
                stats.map_tiles =
                    render_map_into(&mut frame, &self.state().map, self.viewport(), &style);
            }
        }

        self.draw_units(&mut frame, self.viewport(), &mut stats);

        // `BuildMenu` в полноэкранном режиме (без `FRAMEPIC`).
        self.draw_build_menu(&mut frame);
        // Окно сохранения/загрузки (P3) — модальное, как `AbstractMenu` в C++.
        self.draw_save_load(&mut frame);
        // Экран апгрейдов (P6).
        self.draw_upgrade_menu(&mut frame);
        // Экран исследований (P7) и окно помощи поверх него.
        self.draw_research_menu(&mut frame);
        self.draw_help(&mut frame);

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
                stats.resource_icons = render_resources(
                    &mut map,
                    assets,
                    &self.state().map,
                    &viewport,
                    &style,
                    self.state().player_team.0,
                    self.hud().panels.survey,
                );
            }
            _ => {
                stats.map_tiles = render_map_into(&mut map, &self.state().map, &viewport, &style);
            }
        }

        self.draw_units(&mut map, &viewport, &mut stats);
        // Предпросмотр маршрута (зелёная линия RMB) — поверх юнитов, как
        // `GameManager_DrawMapView`: в оконном режиме `render_hud` не
        // вызывается (панели рисует `FRAMEPIC`), поэтому без этой строки
        // предпросмотр был виден только в полноэкранном fallback.
        if let Some(preview) = &self.hud.preview {
            crate::hud::draw_path_preview(&mut map, &viewport, preview);
        }
        crate::chrome::blit_map(&mut frame, &map);

        // Панели поверх карты: кнопки, портрет, статистика, сетка, зум,
        // координаты — как `GameManager_Draw*` в оригинале.
        if let Some(panels) = self.assets().and_then(|assets| assets.panels.as_ref()) {
            panels.draw(&mut frame, self);
        }

        // `BuildMenu` — модальный экран поверх кадра (C++ `AbstractMenu`
        // закрывает главное меню: `GameManager_DisableMainMenu`).
        self.draw_build_menu(&mut frame);
        // Окно сохранения/загрузки (P3).
        self.draw_save_load(&mut frame);
        // Экран апгрейдов (P6).
        self.draw_upgrade_menu(&mut frame);
        // Экран исследований (P7) и окно помощи поверх него.
        self.draw_research_menu(&mut frame);
        self.draw_help(&mut frame);

        (frame, stats)
    }

    /// Рисует открытый экран апгрейдов поверх кадра (P6).
    pub(crate) fn draw_upgrade_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.upgrade_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state());
    }

    /// Рисует открытый экран исследований поверх кадра (P7).
    pub(crate) fn draw_research_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.research_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state());
    }

    /// Рисует открытое окно помощи поверх кадра (P7).
    ///
    /// Палитра — палитра окна (`FRAMEPIC`), как у остальных модальных окон;
    /// если хрома нет — палитра меню.
    pub(crate) fn draw_help(&self, frame: &mut RgbaBuffer) {
        let (Some(help), Some(assets)) = (self.help_screen(), self.assets()) else {
            return;
        };
        let fallback = UiPalette::from_assets(assets)
            .or_else(|| assets.menu.as_ref().map(|menu| menu.palette));

        help.draw(frame, assets.help.as_ref(), fallback);
    }

    /// Рисует открытое окно сохранения/загрузки поверх кадра (P3).
    pub(crate) fn draw_save_load(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.save_load(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets);
    }

    /// Рисует открытый `BuildMenu` поверх кадра (P2).
    pub(crate) fn draw_build_menu(&self, frame: &mut RgbaBuffer) {
        let (Some(menu), Some(assets)) = (self.build_menu(), self.assets()) else {
            return;
        };

        menu.draw(frame, assets, self.state(), self.hud().mouse_position);
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
        // Спрайт юнита может свисать в кадр из-за края, даже если его клетка
        // уже за границей видимого диапазона: берём запас в 2 клетки.
        let range = viewport.visible_tiles();
        let margin = 2;

        for id in self.state().unit_ids_in_order() {
            let Some(unit) = self.state().unit(id) else {
                stats.units_skipped += 1;
                continue;
            };
            let in_range = range.is_some_and(|range| {
                unit.grid.x >= range.x0 - margin
                    && unit.grid.y >= range.y0 - margin
                    && unit.grid.x <= range.x1 + margin
                    && unit.grid.y <= range.y1 + margin
            });
            if !in_range {
                continue;
            }
            if !unit.is_visible_to_team(self.state().player_team) {
                continue;
            }
            // `UnitInfoGroup::IsRelevant` (`unitinfogroup.cpp:59`): idle-юниты
            // не рисуются (кроме QuickBuild-превью, которого у нас нет).
            if unit.orders == max_core::enums::UnitOrder::Idle {
                stats.units_skipped += 1;
                continue;
            }
            positions.push((unit.grid.x, unit.grid.y));
            visible.push(id);
        }

        for index in draw_order(&positions) {
            let Some(unit) = self.state().unit(visible[index]) else {
                continue;
            };
            // Визуальная позиция — дробная клетка, догоняющая симуляцию
            // (см. `App::update_visual_units`). Якорь — центр клетки в мировых
            // пикселях, затем камера переводит его в экранные.
            let (grid_x, grid_y) = self
                .visual_position(visible[index])
                .unwrap_or((f32::from(unit.grid.x), f32::from(unit.grid.y)));
            let world_x = (grid_x * 64.0 + 32.0).round() as i32;
            let world_y = (grid_y * 64.0 + 32.0).round() as i32;
            let anchor = viewport.world_to_screen(world_x, world_y);

            let drawn = self
                .assets()
                .and_then(|assets| draw_unit_sprites(assets, unit, frame, anchor, viewport.zoom()));

            match drawn {
                Some(_) => stats.units_drawn += 1,
                None => {
                    draw_marker(frame, anchor, unit.team.0, viewport.zoom());
                    stats.units_marker += 1;
                }
            }
        }
    }
}

/// Системная палитра окна для simple image (маркеры ресурсов, курсоры).
///
/// В игре это палитра `FRAMEPIC` (её C++ ставит системной при загрузке хрома);
/// без хрома — `COLOR.PAL` как разумный fallback.
pub fn window_palette(assets: &crate::assets::Assets) -> PaletteRgba {
    if let Some(chrome) = assets.chrome.as_ref() {
        return UiPalette::from_big_image(&chrome.frame).palette_rgba();
    }

    let mut palette = [[0u8, 0, 0, 255]; 256];

    for (index, entry) in palette.iter_mut().enumerate() {
        let [r, g, b] = assets.palette.rgb(index as u8);

        *entry = [r, g, b, 255];
    }

    palette
}

/// Маркеры ресурсов: настоящие `RAWMSK*`/`FUELMK*`/`GOLDMK*`, иначе точки.
#[allow(clippy::too_many_arguments)]
fn render_resources(
    frame: &mut RgbaBuffer,
    assets: &crate::assets::Assets,
    map: &max_core::map::Map,
    viewport: &max_render::viewport::Viewport,
    style: &TileStyle,
    team: u8,
    show_all: bool,
) -> u32 {
    if assets.resource_markers.is_ready() {
        max_render::map_render::render_resource_icons_into(
            frame,
            map,
            viewport,
            &assets.resource_markers,
            &window_palette(assets),
            team,
            show_all,
        )
    } else {
        max_render::map_render::render_resources_into(frame, map, viewport, style)
    }
}

/// Маркер юнита 6×6 в точке `anchor` (центр клетки), масштабируется вместе
/// с картой (`zoom`, 64 — 1:1).
fn draw_marker(frame: &mut RgbaBuffer, anchor: (i32, i32), team: u8, zoom: u32) {
    let color = MARKER_COLORS
        .get(usize::from(team))
        .copied()
        .unwrap_or(MARKER_COLORS[4]);
    let radius = ((6 * i64::from(zoom.max(1))) / 64).max(2) as i32;

    for y in -radius..=radius {
        for x in -radius..=radius {
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
    fn enemy_units_are_rendered_only_when_visible() {
        use max_core::enums::UnitType;
        use max_core::geo::Point;
        use max_core::team::TeamId;

        let mut app = App::new(AppConfig::default());
        *app.state_mut() = max_core::game::GameState::new();

        let own = app
            .state_mut()
            .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(55, 55), 0)
            .expect("свой танк");
        let enemy = app
            .state_mut()
            .spawn_unit(UnitType::Tank, TeamId::GREEN, Point::new(56, 55), 0)
            .expect("чужой танк");

        assert!(app
            .state()
            .unit(own)
            .unwrap()
            .is_visible_to_team(TeamId::RED));
        assert!(
            !app.state()
                .unit(enemy)
                .unwrap()
                .is_visible_to_team(TeamId::RED),
            "чужой юнит вне FOV не виден"
        );

        // Без ассетов юниты рисуются маркерами: свой — один.
        let (frame, stats) = app.render_frame_with_stats();
        assert_eq!(stats.units_marker, 1, "виден только свой юнит");
        assert_eq!(stats.units_drawn, 0);
        assert_eq!(
            frame.get(
                55 * 64 + 32 - app.viewport().origin().0,
                55 * 64 + 32 - app.viewport().origin().1
            ),
            Some(MARKER_COLORS[0]),
            "свой маркер на карте"
        );

        // Появился в FOV (разведка/радар) — рисуется и чужой.
        app.state_mut()
            .unit_mut(enemy)
            .unwrap()
            .set_visible_to_team(TeamId::RED, true);
        let (_, stats) = app.render_frame_with_stats();
        assert_eq!(stats.units_marker, 2, "видимый чужой юнит рисуется");
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
