//! M-HS: hot-seat (2–4 человека на одном ПК) — экран перехода хода и тексты.
//!
//! # Что делает оригинал (`max-cpp-port/src/game_manager.cpp`)
//!
//! Hot-seat в C++ — это не отдельный режим симуляции, а следствие того, что
//! «человеческих» команд больше одной:
//!
//! 1. `GameManager_UpdateHumanPlayerCount` (`:3267`) считает команды с
//!    `TEAM_TYPE_PLAYER`; если их `> 1`, счётчик остаётся ненулевым и
//!    `GameManager_PlayMode` **принудительно** становится `PLAY_MODE_TURN_BASED`
//!    (в одиночной игре счётчик обнуляется);
//! 2. `GameManager_TeamTurnTurnBased` (`:451`) идёт по командам RED → GREEN →
//!    BLUE → GRAY и на каждую играющую команду вызывает
//!    `GameManager_UpdateGui(team, ...)` (`:2407`);
//! 3. в ветке `TEAM_TYPE_PLAYER` (`:2437`) при смене команды
//!    (`GameManager_PlayerTeam != team`) и ненулевом `GameManager_HumanPlayerCount`
//!    вызывается `GameManager_UpdateGuiControl(team)` (`:2262`). Он
//!    **показывает модальное сообщение**:
//!
//!    ```c
//!    sprintf(message, _(c2eb), menu_team_names[team]);
//!    MessageManager_DrawMessage(message, MESSAGE_BOX_INFO, MESSAGE_BOX_MODAL, true);
//!    ```
//!
//!    `_(c2eb)` = `"%s:\nBegin turn."`, `menu_team_names[]` (`_(f394)`, `_(a8a6)`,
//!    `_(a3ee)`, `_(319d)`) = `Red Team`, `Green Team`, `Blue Team`, `Gray Team`.
//!    Модальный (`MESSAGE_BOX_MODAL`) путь `MessageManager_DrawMessage`
//!    (`message_manager.cpp:144`) — это `DialogMenu dialog(text, true); dialog.Run()`;
//! 4. `DialogMenu` (`dialogmenu.cpp:145`) рисует окно `HELPFRAM` (301×231),
//!    отцентрованное в `WINDOW_MAIN_MAP` (`GameManager_GetDialogWindowCenterMode`,
//!    `dialog_center_mode = 0` по умолчанию → `(253,126)`), текст `GNW_TEXT_FONT_5`
//!    цветом `GNW_TEXT_OUTLINE | 0xFF` в области `(20,14)` 265×175 (по центру
//!    по горизонтали, блок — по центру по вертикали), кнопку `HELPOK_U/D` 65×24
//!    в `(120,193)` с подписью `OK`; выход — клик по кнопке, **Enter** или **Esc**
//!    (`ProcessKey`, `:103`). Пока диалог открыт, игровой цикл стоит в
//!    `DialogMenu::Run()`, а таймер хода ещё не запущен
//!    (`GameManager_UpdateGui` включает его после возврата из `UpdateGuiControl`,
//!    `:2490`);
//! 5. после подтверждения `GameManager_PlayerTeam = team`, `Access_UpdateMinimapFogOfWar(team, …)`
//!    и `GameManager_UpdatePanelButtons(team)` — ввод и панели переключаются на
//!    нового игрока.
//!
//! Другими словами: **в оригинале экран перехода есть**, но это не отдельный
//! экран, а модальное окно `DialogMenu` с текстом «<Команда>: Begin turn.».
//! Здесь оно воспроизведено 1:1 (ресурсы, координаты, шрифт, цвет, клавиши);
//! отличий от оракула — только отсутствие прокрутки (`BLDUP/BLDDWN`) для
//! текстов длиннее 17 строк: сообщение — две строки.
//!
//! # Настройка
//!
//! `menu_custom_game_menu(false)` (`menu.cpp:2115`) — hot-seat вариант меню
//! настройки: те же три экрана (`GameConfigMenu` → `PlanetSelectMenu` →
//! `ChoosePlayerMenu`), но `ChoosePlayerMenu` получает `is_single_player =
//! false`, поэтому:
//!
//! * заголовок экрана — `_(55c8)` = `Hot Seat Menu` (вместо `_(5512)` =
//!   `Custom Game Menu`);
//! * «Human» на другом слоте **не переносит** флаг человека
//!   (`EventSelectHuman`, `:79`), так что людей можно выбрать несколько;
//! * стартовые значения `menu_choose_player_menu_loop` (`menu.cpp:1883`):
//!   RED — человек, GREEN — человек, BLUE/GRAY — пусто;
//! * `EventDone` (`:212`) требует минимум **двух людей**: `_(06ec)` =
//!   `There must be at least two human players to begin a hot seat game.`;
//! * имена команд в партии — `Red Team`/`Green Team`/… (`menu.cpp:1941`:
//!   `player_name` подставляется только в одиночной игре).
//!
//! Точка входа в оригинале — `Multiplayer Menu` (`menu_multiplayer_menu_loop`,
//! `:2361`) → кнопка `Hot Seat Game` (`_(f731)`).

use max_core::team::{TeamId, TeamKind};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw_centered_shadowed, draw_text_gnw_shadowed, GnwFontId};

use crate::local::LOCAL_TEAM_COUNT;
use crate::menu::{blit_simple, draw_big_image_at, wrap_gnw_text, MenuAssets};
use crate::panels::UiPalette;

/// Заголовок экрана команд в одиночной игре (`_(5512)`).
pub const TITLE_SINGLE: &str = "Custom Game Menu";
/// Заголовок экрана команд в hot-seat (`_(55c8)`).
pub const TITLE_HOT_SEAT: &str = "Hot Seat Menu";

/// `_(bc94)`: нужно минимум два участника (`ChoosePlayerMenu::EventDone`).
pub const MSG_TWO_PLAYERS: &str = "There must be at least two players to begin a new game.";
/// `_(06ec)`: нужно минимум два человека (hot-seat).
pub const MSG_TWO_HUMANS: &str =
    "There must be at least two human players to begin a hot seat game.";

/// Имена команд в hot-seat — `menu_team_names[]` (`_(f394)`, `_(a8a6)`, `_(a3ee)`,
/// `_(319d)`).
pub const TEAM_NAMES: [&str; LOCAL_TEAM_COUNT] =
    ["Red Team", "Green Team", "Blue Team", "Gray Team"];

/// Сообщение перехода хода (`_(c2eb)` = `"%s:\nBegin turn."`).
pub fn turn_message(team: TeamId) -> String {
    format!("{}:\nBegin turn.", team_display_name(team))
}

/// Имя команды для сообщения/экрана.
pub fn team_display_name(team: TeamId) -> &'static str {
    TEAM_NAMES.get(usize::from(team.0)).copied().unwrap_or("")
}

// ---------------------------------------------------------------------------
// Раскладка DialogMenu (`HELPFRAM` + `HELPOK`)
// ---------------------------------------------------------------------------

/// `HELPFRAM` — фон диалога 301×231 (`MAX.RES`; в C++ `DLG_...`/`HELPFRM`).
pub const HELPFRAM_SIZE: (i32, i32) = (301, 231);
/// `WINDOW_MAIN_MAP` — область карты 448×448 в окне 640×480 (`window_manager.cpp`).
pub const MAIN_MAP_RECT: PixelRect = PixelRect::new(180, 18, 448, 448);
/// Окно диалога: `HELPFRAM`, отцентрованный в `WINDOW_MAIN_MAP`
/// (`Window(HELPFRM, GameManager_GetDialogWindowCenterMode())`).
pub const DIALOG_RECT: PixelRect = PixelRect::new(253, 126, 301, 231);
/// Область текста: `canvas = Image(20, 14, 265, 175)` (`dialogmenu.cpp:159`).
pub const TEXT_RECT: PixelRect = PixelRect::new(20, 14, 265, 175);
/// Кнопка `OK` (`new Button(HELPOK_U, HELPOK_D, 120, 193)`, `:162`) в координатах окна.
pub const OK_RECT: PixelRect = PixelRect::new(120, 193, 65, 24);
/// Текст `GNW_TEXT_OUTLINE | 0xFF` (`MessageManager_DrawMessageBoxText`).
pub const TEXT_COLOR: u8 = 0xFF;
/// Цвет подписи кнопки `OK` — `Fonts_GoldColor` (`SetCaption` по умолчанию).
pub const CAPTION_GOLD: (u8, u8, u8) = (165, 175, 199);

/// Окно диалога в координатах кадра.
pub const fn dialog_rect_frame() -> PixelRect {
    DIALOG_RECT
}

/// Область текста в координатах кадра.
pub fn text_rect_frame() -> PixelRect {
    PixelRect::new(
        DIALOG_RECT.ulx + TEXT_RECT.ulx,
        DIALOG_RECT.uly + TEXT_RECT.uly,
        TEXT_RECT.width,
        TEXT_RECT.height,
    )
}

/// Кнопка `OK` в координатах кадра.
pub fn ok_rect_frame() -> PixelRect {
    PixelRect::new(
        DIALOG_RECT.ulx + OK_RECT.ulx,
        DIALOG_RECT.uly + OK_RECT.uly,
        OK_RECT.width,
        OK_RECT.height,
    )
}

/// Попадание в кнопку `OK` (координаты кадра 640×480).
pub fn ok_at(x: i32, y: i32) -> bool {
    ok_rect_frame().contains(x, y)
}

// ---------------------------------------------------------------------------
// Экран перехода хода
//
// «Модальное окно» состояния: `LocalGame::notice` хранит запись `TurnNotice`,
// игровой цикл её не тикает, а ввод принимает только подтверждение.
// ---------------------------------------------------------------------------

/// Экран перехода хода: «<Команда>: Begin turn.» (`GameManager_UpdateGuiControl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnNotice {
    /// Чья команда начинает ход.
    pub team: TeamId,
}

impl TurnNotice {
    /// Запись для команды `team`.
    pub const fn new(team: TeamId) -> Self {
        Self { team }
    }

    /// Текст окна (`_(c2eb)` + `menu_team_names[team]`).
    pub fn message(&self) -> String {
        turn_message(self.team)
    }

    /// Строки окна: `Text_SplitText` режет по `\n`, затем по ширине 265.
    pub fn lines(&self) -> Vec<String> {
        wrap_gnw_text(&self.message(), TEXT_RECT.width)
    }
}

/// Рисует окно перехода хода поверх кадра.
///
/// Раскладка — 1:1 `DialogMenu::DrawText` (`dialogmenu.cpp:31`):
/// * фон `HELPFRAM` в `(253,126)`;
/// * строки шрифтом `GNW_TEXT_FONT_5`, цвет `0xFF` с чёрной обводкой,
///   шаг 10 px, блок по центру области 265×175, каждая строка — по центру;
/// * кнопка `HELPOK_U/D` 65×24 в `(373,319)` с подписью `OK`.
pub fn draw_turn_notice(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    palette: &UiPalette,
    notice: &TurnNotice,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let mut painted = 0;

    if let Some(helpfram) = menu.helpfram.as_ref() {
        painted += draw_big_image_at(
            frame,
            helpfram,
            palette,
            (DIALOG_RECT.ulx, DIALOG_RECT.uly),
            DIALOG_RECT,
        );
    } else {
        // Ресурса нет (урезанный `MAX.RES`): честная рамка вместо окна.
        frame.fill_rect(DIALOG_RECT, palette.rgba(0x22));
    }

    let font = GnwFontId::Font5.font();
    let row_height = font.height();
    let lines = notice.lines();
    let block = row_height * lines.len() as i32;
    let text = text_rect_frame();
    let mut y = text.uly + ((TEXT_RECT.height - block).max(0)) / 2;

    for line in &lines {
        let width = font.text_width(line);
        let x = text.ulx + ((TEXT_RECT.width - width).max(0)) / 2;

        painted += draw_text_gnw_shadowed(
            frame,
            x,
            y,
            font,
            line,
            palette.rgba(TEXT_COLOR),
            palette.rgba(0),
            palette.rgba(0),
        )
        .max(0) as u32;
        y += row_height;
    }

    // Кнопка `OK`: `HELPOK_U`/`HELPOK_D`, подпись `_(ae0b)` = "OK".
    let ok = ok_rect_frame();
    let hovered = mouse.is_some_and(|(x, y)| ok.contains(x, y));

    if let Some(pair) = menu.helpok.as_ref() {
        painted += blit_simple(frame, palette, pair.image(hovered), ok.ulx, ok.uly, true);
    } else {
        frame.fill_rect(ok, palette.rgba(0x17));
    }

    // `Button::SetCaption(caption, 2, 2)` → область (0,0) 63×22, текст по центру
    // на `((22 - 10) / 2) = 6` от верха кнопки.
    let caption_rect = PixelRect::new(ok.ulx, ok.uly + 6, ok.width - 2, font.height());
    let (base, outline, shadow) = if hovered {
        // `Fonts_DarkOrageColor` — нажатое состояние.
        (5u8, 56u8, 199u8)
    } else {
        CAPTION_GOLD
    };

    painted += draw_text_gnw_centered_shadowed(
        frame,
        caption_rect,
        font,
        "OK",
        palette.rgba(base),
        palette.rgba(outline),
        palette.rgba(shadow),
    )
    .max(0) as u32;

    painted
}

/// Команды-люди в порядке слотов (`GameManager_UpdateHumanPlayerCount`).
pub fn human_teams(team_types: &[TeamKind; LOCAL_TEAM_COUNT]) -> Vec<TeamId> {
    team_types
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == TeamKind::Player)
        .map(|(index, _)| TeamId(index as u8))
        .collect()
}

/// Строка перехода для журнала/статуса.
pub fn notice_log_line(notice: &TurnNotice) -> String {
    format!("{}: begin turn", team_display_name(notice.team))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_is_centered_in_the_main_map() {
        // `((627 - 180 - 301) / 2) + 180 = 253`, `((465 - 18 - 231) / 2) + 18 = 126`.
        assert_eq!(DIALOG_RECT, PixelRect::new(253, 126, 301, 231));
        assert_eq!(
            MAIN_MAP_RECT.ulx + (MAIN_MAP_RECT.width - HELPFRAM_SIZE.0) / 2,
            253
        );
        assert_eq!(
            MAIN_MAP_RECT.uly + (MAIN_MAP_RECT.height - HELPFRAM_SIZE.1) / 2,
            126
        );
    }

    #[test]
    fn ok_button_matches_the_dialog_menu_controls() {
        // `Button(HELPOK_U, HELPOK_D, 120, 193)` 65×24 внутри окна диалога.
        assert_eq!(OK_RECT, PixelRect::new(120, 193, 65, 24));
        assert_eq!(ok_rect_frame(), PixelRect::new(373, 319, 65, 24));
        assert!(ok_at(380, 325));
        assert!(!ok_at(253, 126), "угол окна — не кнопка");
    }

    #[test]
    fn turn_message_matches_the_oracle_string() {
        assert_eq!(turn_message(TeamId::RED), "Red Team:\nBegin turn.");
        assert_eq!(turn_message(TeamId::GREEN), "Green Team:\nBegin turn.");
        assert_eq!(turn_message(TeamId::BLUE), "Blue Team:\nBegin turn.");
        assert_eq!(turn_message(TeamId::GRAY), "Gray Team:\nBegin turn.");

        let notice = TurnNotice::new(TeamId::GREEN);
        assert_eq!(notice.lines(), vec!["Green Team:", "Begin turn."]);
    }

    #[test]
    fn text_block_is_vertically_centered() {
        // Две строки по 10 px → `(175 - 20) / 2 = 77` от верха области текста.
        let text = text_rect_frame();
        let block = 10 * TurnNotice::new(TeamId::RED).lines().len() as i32;
        let y = text.uly + (TEXT_RECT.height - block).max(0) / 2;

        assert_eq!(text, PixelRect::new(273, 140, 265, 175));
        assert_eq!(y, 140 + 77);
    }

    #[test]
    fn human_teams_follow_the_team_slot_order() {
        let teams = human_teams(&[
            TeamKind::Player,
            TeamKind::Computer,
            TeamKind::Player,
            TeamKind::None,
        ]);

        assert_eq!(teams, vec![TeamId::RED, TeamId::BLUE]);
    }
}
