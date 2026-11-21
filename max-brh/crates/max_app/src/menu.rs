//! Меню на оригинальных ресурсах `MAX.RES` (пункт E `max-brh-docs/visual-1to1-status.md`).
//!
//! C++-порт рисует меню не процедурно, а готовыми картинками и растровым
//! шрифтом GNW:
//!
//! * **главное меню** (`menu.cpp::main_menu`) — фон `MAINPIC` 640×480 (в нём уже
//!   нарисован логотип M.A.X.), заголовок `Main Menu` шрифтом `FONT5` в
//!   (236,145) 172×15, «портрет» `P_*` 300×240 в (16,182) в рамках
//!   `BGRSCRNL`/`BGRSCRNR` (6,173)/(166,173), шесть кнопок `BLANK_UP` 200×29 на
//!   x=385 с шагом 35 от y=175 и `Exit` `SBLNK_UP` 100×30 в (435,420),
//!   копирайт внизу;
//! * **меню настройки/выбора планеты** (`gamesetupmenu.cpp`,
//!   `planetselectmenu.cpp`) — фон `PLANETSE`/`MAINPIC`, кнопки `MNUBTN4U`/
//!   `MNUBTN5U`/`MNUBTN6U` 109×40/48×40 в нижней строке y=438;
//! * **список игр** (`networkmenu.cpp`) — фон `MULTGAME`, строки хостов
//!   (359,28) 209×160, по 20 px, разделители `COLOR_GREEN`, кнопки
//!   `MNUBTN3U`/`MNUBTN4U`/`MNUBTN5U`/`MNUBTN6U` в (243/354/465/514, 438).
//!
//! Подписи кнопок — `GNW_TEXT_FONT_1`, цвета `Fonts_GoldColor (165,175,199)` и
//! `Fonts_DarkOrageColor (5,56,199)`; простые картинки индексируются системной
//! палитрой окна, то есть палитрой `MAINPIC` (C++ загружает её с
//! `palette_from_image=true`).
//!
//! # Отличия от C++
//!
//! * Главное меню показывает все семь кнопок оригинала; действия, которых в
//!   Rust-клиенте ещё нет (`Load Game`, `Setup`, `Introduction`, `Credits`),
//!   пишут строку статуса «экран не портирован» — как кнопки панелей в
//!   `panels.rs`.
//! * Наведение мыши **не** переключает кнопку на `*D`: нажатая картинка
//!   остаётся только у выбранных radio/toggle-кнопок (как `Button::SetRestState`
//!   в C++); у обычных кнопок `*D` — лишь на время реального нажатия.
//! * Сетка планет 8×3 оставлена из Rust-клиента (в оригинале 3×2 на экране
//!   `PLANETSE`), но вписана в область 3×2-слотов `PLANETSE` (192,51) 388×249.
//! * `BTNHIDE` в C++-порте не используется (проверено `grep`); здесь он —
//!   кнопка «скрыть список» в правом верхнем углу рамки списка игр.

use max_assets::image::{BigImage, SimpleImage};
use max_render::buffer::{PixelRect, Rgba, RgbaBuffer};
use max_render::text::{
    draw_text_gnw, draw_text_gnw_centered, draw_text_gnw_centered_shadowed, draw_text_gnw_shadowed,
    GnwFontId,
};
use max_render::ui::Rect;

use crate::panels::UiPalette;
use crate::ui::{planet_name, CreateField, CreateForm, PLANET_COUNT};
use crate::Assets;

/// Логический размер меню (окно оригинала).
pub const MENU_WIDTH: i32 = 640;
/// Логический размер меню (окно оригинала).
pub const MENU_HEIGHT: i32 = 480;

/// Индекс обычного текста в палитре `MAINPIC` (`0xA2`).
pub const TEXT: u8 = 0xA2;
/// Индекс зелёного текста/линий (`COLOR_GREEN`).
pub const GREEN: u8 = 0x02;
/// Копирайт — чёрный (`COLOR_BLACK`).
pub const BLACK: u8 = 0x00;

/// Пара «отжата/нажата».
#[derive(Debug, Clone)]
pub struct MenuButtonPair {
    up: SimpleImage,
    down: SimpleImage,
}

impl MenuButtonPair {
    /// Пара из готовых картинок (для экранов, которые грузят свои ресурсы —
    /// например `BuildMenu` с `BLDBLD_U`/`BLD2X_U`/`BLD4X_U`).
    pub fn new(up: SimpleImage, down: SimpleImage) -> Self {
        Self { up, down }
    }

    pub fn image(&self, down: bool) -> &SimpleImage {
        if down {
            &self.down
        } else {
            &self.up
        }
    }

    /// Размер картинки (ширина, высота).
    pub fn size(&self) -> (i32, i32) {
        (self.up.width as i32, self.up.height as i32)
    }

    /// Пиксель отжатой/нажатой картинки (для тестов).
    pub fn pixel(&self, down: bool, x: u32, y: u32) -> Option<u8> {
        self.image(down).pixel(x, y)
    }
}

/// Кнопка главного меню.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainMenuAction {
    /// Создать игру (`New Game`).
    Create,
    /// Список игр (`Multiplayer Game`).
    Join,
    /// Выход (`Exit`).
    Quit,
    /// Экран титров (`Credits` → `menu_credits_menu_loop`).
    Credits,
    /// Вступление (`Introduction` → `Movie_PlayIntro`, MVE `maxint.mve`).
    Introduction,
    /// Экран настроек (`menu_setup_window` → `OptionsMenu(SETUPPIC)`).
    Setup,
    /// Экран, который ещё не портирован.
    Unported(&'static str),
}

/// Одна кнопка главного меню в координатах C++.
#[derive(Debug, Clone, Copy)]
pub struct MainMenuButton {
    /// Прямоугольник в окне 640×480.
    pub rect: PixelRect,
    /// Подпись из `language.json` (en-US).
    pub label: &'static str,
    /// Действие.
    pub action: MainMenuAction,
    /// Большая (`BLANK_UP` 200×29) или маленькая (`SBLNK_UP` 100×30).
    pub big: bool,
}

/// Кнопки главного меню (`menu.cpp::main_menu`, `MENU_BUTTON_DEF`).
pub const MAIN_MENU_BUTTONS: [MainMenuButton; 7] = [
    MainMenuButton {
        rect: PixelRect::new(385, 175, 200, 29),
        label: "New Game",
        action: MainMenuAction::Create,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 210, 200, 29),
        label: "Load Game",
        action: MainMenuAction::Unported("Load Game"),
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 245, 200, 29),
        label: "Multiplayer Game",
        action: MainMenuAction::Join,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 280, 200, 29),
        label: "Setup",
        action: MainMenuAction::Setup,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 315, 200, 29),
        label: "Introduction",
        action: MainMenuAction::Introduction,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(385, 350, 200, 29),
        label: "Credits",
        action: MainMenuAction::Credits,
        big: true,
    },
    MainMenuButton {
        rect: PixelRect::new(435, 420, 100, 30),
        label: "Exit",
        action: MainMenuAction::Quit,
        big: false,
    },
];

/// Заголовок главного меню (`draw_menu_title(window, _(35f9))`).
pub const MAIN_TITLE: PixelRect = PixelRect::new(236, 145, 172, 15);
/// «Портрет» главного меню (`P_*` 300×240).
pub const MAIN_PORTRAIT: PixelRect = PixelRect::new(16, 182, 300, 240);
/// Левая рамка портрета `BGRSCRNL`.
pub const MAIN_PORTRAIT_LEFT: (i32, i32) = (6, 173);
/// Правая рамка портрета `BGRSCRNR`.
pub const MAIN_PORTRAIT_RIGHT: (i32, i32) = (166, 173);
/// Копирайт: (10,469) 620×10 (`draw_copyright_label`).
pub const COPYRIGHT: PixelRect = PixelRect::new(10, 469, 620, 10);
/// Строка статуса Rust-клиента (в оригинале её нет).
pub const MAIN_STATUS: PixelRect = PixelRect::new(10, 452, 370, 12);

// ---------------------------------------------------------------------------
// Multiplayer Menu (`menu_multiplayer_menu_loop`, `menu.cpp:2361`)
// ---------------------------------------------------------------------------

/// Кнопка `Multiplayer Menu`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplayerAction {
    /// `Host Network Game` (`_(c7a9)`).
    Host,
    /// `Join Network Game` (`_(7064)`).
    Join,
    /// `Hot Seat Game` (`_(f731)`) — M-HS.
    HotSeat,
    /// `Hot Seat Scenarios` (`_(bf00)`).
    HotSeatScenarios,
    /// `Load Hot Seat Game` (`_(847e)`).
    LoadHotSeat,
    /// `Cancel` (`_(e13e)`).
    Cancel,
}

/// Кнопка `Multiplayer Menu` в координатах C++.
#[derive(Debug, Clone, Copy)]
pub struct MultiplayerButton {
    /// Прямоугольник (200×29 `BLANK_UP`, либо 100×30 `SBLNK_UP` для `Cancel`).
    pub rect: PixelRect,
    /// Подпись (`language.json`, en-US).
    pub label: &'static str,
    /// Действие.
    pub action: MultiplayerAction,
    /// Большая кнопка (`BLANK_*`) или маленькая (`SBLNK_*`).
    pub big: bool,
}

/// Заголовок `Multiplayer Menu` (`draw_menu_title(window, _(f2b3))`).
pub const MULTIPLAYER_TITLE: PixelRect = PixelRect::new(236, 145, 172, 15);

/// Шесть кнопок `network_game_menu_buttons` (`menu.cpp:2372`).
pub const MULTIPLAYER_BUTTONS: [MultiplayerButton; 6] = [
    MultiplayerButton {
        rect: PixelRect::new(385, 175, 200, 29),
        label: "Host Network Game",
        action: MultiplayerAction::Host,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 210, 200, 29),
        label: "Join Network Game",
        action: MultiplayerAction::Join,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 280, 200, 29),
        label: "Hot Seat Game",
        action: MultiplayerAction::HotSeat,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 315, 200, 29),
        label: "Hot Seat Scenarios",
        action: MultiplayerAction::HotSeatScenarios,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(385, 350, 200, 29),
        label: "Load Hot Seat Game",
        action: MultiplayerAction::LoadHotSeat,
        big: true,
    },
    MultiplayerButton {
        rect: PixelRect::new(435, 420, 100, 30),
        label: "Cancel",
        action: MultiplayerAction::Cancel,
        big: false,
    },
];

/// Кнопка `Multiplayer Menu` под точкой `(x, y)`.
pub fn multiplayer_button_at(x: i32, y: i32) -> Option<MultiplayerAction> {
    MULTIPLAYER_BUTTONS
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| button.action)
}

/// Кнопка нижней строки меню настройки/планет.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuButtonAction {
    /// Случайная планета.
    Random,
    /// Назад/отмена.
    Cancel,
    /// Помощь (экран не портирован).
    Help,
    /// Готово/создать.
    Done,
}

/// Кнопка нижней строки в координатах экрана.
#[derive(Debug, Clone, Copy)]
pub struct ActionButton {
    /// Прямоугольник.
    pub rect: PixelRect,
    /// Подпись.
    pub label: &'static str,
    /// Действие.
    pub action: MenuButtonAction,
    /// Номер ресурса `MNUBTNnU` (1..=6).
    pub resource: usize,
}

/// Кнопки `PlanetSelectMenu` (`planetselectmenu.hpp`, y=438).
pub const PLANET_BUTTONS: [ActionButton; 4] = [
    ActionButton {
        rect: PixelRect::new(243, 438, 109, 40),
        label: "Random",
        action: MenuButtonAction::Random,
        resource: 4,
    },
    ActionButton {
        rect: PixelRect::new(354, 438, 109, 40),
        label: "Cancel",
        action: MenuButtonAction::Cancel,
        resource: 4,
    },
    ActionButton {
        rect: PixelRect::new(465, 438, 48, 40),
        label: "?",
        action: MenuButtonAction::Help,
        resource: 5,
    },
    ActionButton {
        rect: PixelRect::new(514, 438, 109, 40),
        label: "Done",
        action: MenuButtonAction::Done,
        resource: 6,
    },
];

/// Заголовок экрана планет (`_(f7c8)` = "Planet Select Menu").
pub const PLANET_TITLE: PixelRect = PixelRect::new(230, 6, 180, 20);
/// Имя выбранной планеты (16,61) 149×19.
pub const PLANET_NAME: PixelRect = PixelRect::new(16, 61, 149, 19);
/// Область 3×2 слотов `PLANETSE`, в которую вписана сетка 8×3.
pub const PLANET_GRID: PixelRect = PixelRect::new(192, 51, 388, 249);
/// Описание/поля формы (41,350) 559×59.
pub const PLANET_DESCRIPTION: PixelRect = PixelRect::new(41, 350, 559, 59);
/// Строка статуса формы.
pub const PLANET_STATUS: PixelRect = PixelRect::new(41, 412, 559, 12);

/// Прямоугольники шести полей формы (3×2 внутри описания `PLANETSE`).
pub const CREATE_FIELD_RECTS: [(CreateField, PixelRect); 6] = [
    (CreateField::Name, PixelRect::new(48, 352, 180, 24)),
    (CreateField::Timer, PixelRect::new(237, 352, 180, 24)),
    (CreateField::EndTurn, PixelRect::new(426, 352, 174, 24)),
    (CreateField::Opponent, PixelRect::new(48, 378, 180, 24)),
    (CreateField::Password, PixelRect::new(237, 378, 180, 24)),
    (CreateField::World, PixelRect::new(426, 378, 174, 24)),
];

/// Кнопка списка игр.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinButton {
    /// Обновить список (`Options` в C++).
    Refresh,
    /// Назад (`Cancel`).
    Back,
    /// Помощь (не портирована).
    Help,
    /// Войти (`Ready`).
    Join,
    /// Скрыть список (`BTNHIDE`).
    Hide,
}

/// Кнопки `NetworkMenu` (243/354/465/514, 438).
pub const JOIN_BUTTONS: [ActionButton; 4] = [
    ActionButton {
        rect: PixelRect::new(243, 438, 109, 40),
        label: "Options",
        action: MenuButtonAction::Random,
        resource: 3,
    },
    ActionButton {
        rect: PixelRect::new(354, 438, 109, 40),
        label: "Cancel",
        action: MenuButtonAction::Cancel,
        resource: 4,
    },
    ActionButton {
        rect: PixelRect::new(465, 438, 48, 40),
        label: "?",
        action: MenuButtonAction::Help,
        resource: 5,
    },
    ActionButton {
        rect: PixelRect::new(514, 438, 109, 40),
        label: "Ready",
        action: MenuButtonAction::Done,
        resource: 6,
    },
];

/// Рамка/область списка игр (`MENU_ITEM_JOIN_TEXT_WINDOW`, (359,28) 209×160).
pub const JOIN_LIST: PixelRect = PixelRect::new(359, 28, 209, 160);
/// Высота строки списка (20 px).
pub const JOIN_ROW_HEIGHT: i32 = 20;
/// Панель под списком (на 4 px шире — «рамка»).
pub const JOIN_PANEL: PixelRect = PixelRect::new(355, 24, 217, 168);
/// Кнопка `BTNHIDE` в правом верхнем углу списка.
pub const JOIN_HIDE: PixelRect = PixelRect::new(528, 4, 38, 21);
/// Заголовок списка игр.
pub const JOIN_TITLE: PixelRect = PixelRect::new(359, 4, 190, 16);
/// Строка статуса списка.
pub const JOIN_STATUS: PixelRect = PixelRect::new(150, 400, 462, 15);

/// Сколько строк списка видно одновременно.
pub const JOIN_VISIBLE_ROWS: usize = 8;

/// Первая видимая строка при выборе `selected`.
pub fn join_scroll(selected: usize) -> usize {
    selected.saturating_sub(JOIN_VISIBLE_ROWS - 1)
}

/// Прямоугольник строки списка (0 — первая видимая).
pub fn join_row_rect(row: usize) -> PixelRect {
    if row >= JOIN_VISIBLE_ROWS {
        return PixelRect::new(0, 0, 0, 0);
    }

    PixelRect::new(
        JOIN_LIST.ulx,
        JOIN_LIST.uly + row as i32 * JOIN_ROW_HEIGHT,
        JOIN_LIST.width,
        JOIN_ROW_HEIGHT,
    )
}

/// Строка списка под точкой (с учётом прокрутки); `None` — вне списка.
pub fn join_row_at(x: i32, y: i32, games: usize, selected: usize) -> Option<usize> {
    if !JOIN_LIST.contains(x, y) {
        return None;
    }

    let row = ((y - JOIN_LIST.uly) / JOIN_ROW_HEIGHT) as usize;
    let index = join_scroll(selected) + row;

    (row < JOIN_VISIBLE_ROWS && index < games).then_some(index)
}

/// Загруженные ресурсы меню.
#[derive(Debug, Clone)]
pub struct MenuAssets {
    /// Фон главного меню 640×480 (`MAINPIC`).
    pub mainpic: BigImage,
    /// Фон экрана планет (`PLANETSE`).
    pub planetse: Option<BigImage>,
    /// Фон списка игр (`MULTGAME`).
    pub multgame: Option<BigImage>,
    /// Фон окна игры (`FRAMEPIC`) — fallback, если чего-то из меню нет.
    pub framepic: Option<BigImage>,
    /// Фон экрана настройки партии (`OPTNFRM`, `GameConfigMenu`).
    pub optnfrm: Option<BigImage>,
    /// Фон экрана выбора команд (`CHOSPLYR`, `ChoosePlayerMenu`).
    pub chosplyr: Option<BigImage>,
    /// Картинки планет по группам (`SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC`).
    pub planet_group_pictures: [Option<BigImage>; 4],
    /// Стрелки смены группы планет (`MNULAROU`, `MNURAROU`).
    pub planet_arrows: Option<(SimpleImage, SimpleImage)>,
    /// Кнопки выбора типа команды: `CH_HUM_U/D`, `CH_CMP_U/D`, `CH_NON_U/D`.
    pub team_kind_buttons: [Option<MenuButtonPair>; 3],
    /// Флаги команд `CH_TM1_U/D` … `CH_TM4_U/D`.
    pub team_banners: [Option<MenuButtonPair>; 4],
    /// Эмблема клана `CLN0LOGO` (42×65): `ChoosePlayerMenu::ButtonInit`
    /// копирует её на флаг команды в (41,40).
    pub clan_logo: Option<SimpleImage>,
    /// Эмблемы кланов `CLN0LOGO`…`CLN8LOGO` (0 — Random).
    pub clan_logos: [Option<SimpleImage>; 9],
    /// Карточки `CH_CN1_U/D`…`CH_CN8_U/D` (91×86) экрана `ClanSelectMenu`.
    pub clan_buttons: [Option<MenuButtonPair>; 8],
    /// Фон экрана выбора клана (`CLANSEL`).
    pub clansel: Option<BigImage>,
    /// Фон экрана мобилизации (`CARGOPIC`, 640×480) — `CargoMenu`.
    pub cargopic: Option<BigImage>,
    /// Кнопка `Buy` экрана мобилизации (`CRGBUY_U/D`, 41×22).
    pub cargo_buy: Option<MenuButtonPair>,
    /// Кнопка `Delete` (`CRGDEL_U/D`, 53×23).
    pub cargo_delete: Option<MenuButtonPair>,
    /// Кнопка `Cancel` (`BLDCAN_U/D`, 62×23).
    pub cargo_cancel: Option<MenuButtonPair>,
    /// Кнопка `Done` (`BLDONE_U/D`, 54×23).
    pub cargo_done: Option<MenuButtonPair>,
    /// Фоны экрана победы/поражения (`ENDGAME1`…`ENDGAME9`).
    pub endgame_backgrounds: Vec<BigImage>,
    /// `ENDARM` — картинка в правом нижнем углу экрана победы.
    pub endarm: Option<SimpleImage>,
    /// Кнопка `OK` экрана победы (`ENDOK_U/D`).
    pub endok: Option<MenuButtonPair>,
    /// `HELPFRAM` — фон окна `DialogMenu` 301×231 (экран перехода хода M-HS).
    pub helpfram: Option<BigImage>,
    /// Кнопка `OK` окна `DialogMenu` (`HELPOK_U/D`, 65×24).
    pub helpok: Option<MenuButtonPair>,
    /// «Портрет» главного меню (`P_TANK` 300×240).
    pub portrait: Option<BigImage>,
    /// Большие кнопки `BLANK_UP/DN` 200×29.
    pub blank: MenuButtonPair,
    /// Маленькие кнопки `SBLNK_UP/DN` 100×30.
    pub small_blank: MenuButtonPair,
    /// `MNUBTN1U/D` … `MNUBTN6U/D` (индекс 0 — ресурс 1).
    pub mnubtn: [MenuButtonPair; 6],
    /// Кнопка `BTNHIDE` 38×21.
    pub btnhide: Option<SimpleImage>,
    /// Левая рамка портрета `BGRSCRNL` 160×259.
    pub portrait_frame_left: Option<SimpleImage>,
    /// Правая рамка портрета `BGRSCRNR` 160×259.
    pub portrait_frame_right: Option<SimpleImage>,
    /// Фон экрана титров `CREDITS` 640×480 (big image со своей палитрой,
    /// `menu_credits_menu_loop`).
    pub credits: Option<BigImage>,
    /// Фон заглушки `Introduction`: `MLOGO` 640×480 (big image), если есть.
    pub mlogo: Option<BigImage>,
    /// Системная палитра окна (из `MAINPIC`).
    pub palette: UiPalette,
}

impl MenuAssets {
    /// Грузит ресурсы меню из `MAX.RES`.
    ///
    /// Возвращает `None`, только если нет `MAINPIC`/`FRAMEPIC` или основных
    /// кнопок: без них меню не собрать. Остальное (`PLANETSE`, `MULTGAME`,
    /// портрет, рамки, `BTNHIDE`) — опционально.
    pub fn load(max_res: &max_assets::ResArchive) -> Option<Self> {
        let read_big = |name: &str| {
            max_res
                .read(name)
                .ok()
                .and_then(|bytes| BigImage::decode(bytes).ok())
        };
        let read_simple = |name: &str| {
            max_res
                .read(name)
                .ok()
                .and_then(|bytes| SimpleImage::decode(bytes).ok())
        };
        let pair = |up: &str, down: &str| -> Option<MenuButtonPair> {
            Some(MenuButtonPair {
                up: read_simple(up)?,
                down: read_simple(down)?,
            })
        };

        let framepic = read_big("FRAMEPIC");
        let mainpic = read_big("MAINPIC").or_else(|| framepic.clone())?;
        let palette = UiPalette::from_big_image(&mainpic);

        let mut mnubtn = Vec::with_capacity(6);
        for index in 1..=6 {
            mnubtn.push(pair(
                &format!("MNUBTN{index}U"),
                &format!("MNUBTN{index}D"),
            )?);
        }

        let portrait = ["P_TANK", "P_DEPOT", "P_HANGAR", "P_TRANSP"]
            .iter()
            .find_map(|name| read_big(name))
            .or_else(|| {
                // Любая `P_*` 300×240, если привычных имён нет.
                max_res
                    .list()
                    .iter()
                    .map(|entry| entry.name.as_str())
                    .find(|name| name.starts_with("P_"))
                    .and_then(read_big)
            });

        Some(Self {
            mainpic,
            planetse: read_big("PLANETSE"),
            multgame: read_big("MULTGAME"),
            framepic,
            optnfrm: read_big("OPTNFRM"),
            chosplyr: read_big("CHOSPLYR"),
            planet_group_pictures: [
                read_big("SNOW_PIC"),
                read_big("CRTR_PIC"),
                read_big("GREN_PIC"),
                read_big("DSRT_PIC"),
            ],
            planet_arrows: read_simple("MNULAROU").zip(read_simple("MNURAROU")),
            team_kind_buttons: [
                pair("CH_HUM_U", "CH_HUM_D"),
                pair("CH_CMP_U", "CH_CMP_D"),
                pair("CH_NON_U", "CH_NON_D"),
            ],
            team_banners: [
                pair("CH_TM1_U", "CH_TM1_D"),
                pair("CH_TM2_U", "CH_TM2_D"),
                pair("CH_TM3_U", "CH_TM3_D"),
                pair("CH_TM4_U", "CH_TM4_D"),
            ],
            clan_logo: read_simple("CLN0LOGO"),
            clan_logos: std::array::from_fn(|index| read_simple(&format!("CLN{index}LOGO"))),
            clan_buttons: std::array::from_fn(|index| {
                pair(
                    &format!("CH_CN{}_U", index + 1),
                    &format!("CH_CN{}_D", index + 1),
                )
            }),
            clansel: read_big("CLANSEL"),
            cargopic: read_big("CARGOPIC"),
            cargo_buy: pair("CRGBUY_U", "CRGBUY_D"),
            cargo_delete: pair("CRGDEL_U", "CRGDEL_D"),
            cargo_cancel: pair("BLDCAN_U", "BLDCAN_D"),
            cargo_done: pair("BLDONE_U", "BLDONE_D"),
            endgame_backgrounds: [
                "ENDGAME1", "ENDGAME2", "ENDGAME3", "ENDGAME4", "ENDGAME5", "ENDGAME6", "ENDGAME7",
                "ENDGAME8", "ENDGAME9",
            ]
            .iter()
            .filter_map(|name| read_big(name))
            .collect(),
            endarm: read_simple("ENDARM"),
            endok: pair("ENDOK_U", "ENDOK_D"),
            helpfram: read_big("HELPFRAM"),
            helpok: pair("HELPOK_U", "HELPOK_D"),
            portrait,
            blank: pair("BLANK_UP", "BLANK_DN")?,
            small_blank: pair("SBLNK_UP", "SBLNK_DN")?,
            mnubtn: mnubtn.try_into().ok()?,
            btnhide: read_simple("BTNHIDE"),
            portrait_frame_left: read_simple("BGRSCRNL"),
            portrait_frame_right: read_simple("BGRSCRNR"),
            credits: read_big("CREDITS"),
            mlogo: read_big("MLOGO"),
            palette,
        })
    }

    /// Фон экрана планет (`PLANETSE`, иначе `FRAMEPIC`, иначе `MAINPIC`).
    pub fn planet_background(&self) -> &BigImage {
        self.planetse
            .as_ref()
            .or(self.framepic.as_ref())
            .unwrap_or(&self.mainpic)
    }

    /// Фон списка игр (`MULTGAME`, иначе `PLANETSE`, иначе `MAINPIC`).
    pub fn join_background(&self) -> &BigImage {
        self.multgame
            .as_ref()
            .or(self.planetse.as_ref())
            .or(self.framepic.as_ref())
            .unwrap_or(&self.mainpic)
    }

    /// Пара `MNUBTNnU/D` по номеру ресурса (1..=6).
    pub fn mnubtn_pair(&self, resource: usize) -> Option<&MenuButtonPair> {
        self.mnubtn.get(resource.checked_sub(1)?)
    }
}

/// Рисует big image в кадр с учётом `ulx/uly` картинки.
pub fn draw_big_image(frame: &mut RgbaBuffer, image: &BigImage) -> u32 {
    let offset_x = i32::from(image.ulx);
    let offset_y = i32::from(image.uly);
    let mut painted = 0;

    for y in 0..image.height as u32 {
        for x in 0..image.width as u32 {
            let Some([r, g, b]) = image.rgb(x, y) else {
                continue;
            };

            if frame.set(offset_x + x as i32, offset_y + y as i32, Rgba::rgb(r, g, b)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Копирует simple image в кадр палитрой окна.
///
/// `transparent` — пропускать пиксели индекса `image.transparent` (C++
/// `WindowManager_LoadSimpleImage(..., has_transparency)`).
pub fn blit_simple(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    x: i32,
    y: i32,
    transparent: bool,
) -> u32 {
    let mut painted = 0;

    for py in 0..image.height as u32 {
        for px in 0..image.width as u32 {
            let Some(index) = image.pixel(px, py) else {
                continue;
            };

            if transparent && index == image.transparent {
                continue;
            }

            if frame.set(x + px as i32, y + py as i32, palette.rgba(index)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Копирует simple image с учётом её собственного `ulx/uly` — точка «якоря».
///
/// Это дословно `WindowManager_DecodeSimpleImage` (`window_manager.cpp:784`):
///
/// ```c
/// ulx -= image->ulx;
/// uly -= image->uly;
/// ```
///
/// Именно так рисуются эмблемы кланов: `Button::Copy(CLNnLOGO, 41, 40)` кладёт
/// картинку в точку `(41,40)` **якоря**, а собственные `ulx/uly` ресурса
/// (`CLN0LOGO` — 21,32; `CLN1LOGO` — 37,28; …) сдвигают её так, что логотип
/// оказывается по центру кнопки. Без этой поправки картинка уезжает вправо-вниз.
pub fn blit_simple_anchored(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    anchor_x: i32,
    anchor_y: i32,
    transparent: bool,
) -> u32 {
    blit_simple(
        frame,
        palette,
        image,
        anchor_x - i32::from(image.ulx),
        anchor_y - i32::from(image.uly),
        transparent,
    )
}

/// Перенос текста по словам шириной GNW-шрифта `FONT5` (`Text_SplitText`).
///
/// Используется экранами настройки (`GameConfigMenu`/`PlanetSelectMenu`) и
/// окном перехода хода M-HS (`DialogMenu::DrawText`): обещанная ширина — в
/// пикселях, перенос — по пробелам, `\n` — жёсткий перевод строки.
pub fn wrap_gnw_text(text: &str, width: i32) -> Vec<String> {
    let font = GnwFontId::Font5.font();
    let mut lines = Vec::new();

    for paragraph in text.split('\n') {
        let mut line = String::new();

        for word in paragraph.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };

            if font.text_width(&candidate) > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                line = word.to_string();
            } else {
                line = candidate;
            }
        }

        lines.push(line);
    }

    lines
}

/// Заменяет типографские символы, которых нет в GNW-шрифтах (`—`, `…`), на
/// ASCII: иначе `draw_text_gnw` рисует вместо них `?`.
pub fn clean_status(text: &str) -> String {
    let mut output = String::with_capacity(text.len());

    for character in text.chars() {
        match character {
            '—' | '–' | '·' => output.push('-'),
            '…' => output.push_str("..."),
            '’' | '‘' => output.push('\''),
            '“' | '”' => output.push('"'),
            other => output.push(other),
        }
    }

    output
}

/// Заголовок меню: `GNW_TEXT_FONT_5`, `COLOR_GREEN`, по центру прямоугольника.
pub fn draw_title(frame: &mut RgbaBuffer, palette: &UiPalette, rect: PixelRect, text: &str) -> i32 {
    draw_text_gnw_centered(
        frame,
        rect,
        GnwFontId::Font5.font(),
        text,
        palette.rgba(GREEN),
    )
}

/// Кнопка меню с картинкой `up/down` и подписью `GNW_TEXT_FONT_1`.
///
/// В оригинале `*D` показывается только пока кнопка **нажата** (`Button` в
/// `GNW`/`Button::Run`). Наведение мыши не является нажатием, поэтому здесь
/// всегда рисуется `up`; нажатое состояние оставлено для radio/toggle-кнопок,
/// которые передают `down` явно. `_down_image` и `_pressed` сохранены в
/// сигнатуре, чтобы не менять все вызовы.
pub fn draw_image_button(
    frame: &mut RgbaBuffer,
    palette: &UiPalette,
    image: &SimpleImage,
    _down_image: &SimpleImage,
    rect: PixelRect,
    label: &str,
    _pressed: bool,
) -> u32 {
    let mut painted = blit_simple(frame, palette, image, rect.ulx, rect.uly, true);

    if !label.is_empty() {
        let (base, outline, shadow) = (165u8, 175u8, 199u8);

        painted += draw_text_gnw_centered_shadowed(
            frame,
            rect,
            GnwFontId::Font1.font(),
            label,
            palette.rgba(base),
            palette.rgba(outline),
            palette.rgba(shadow),
        )
        .max(0) as u32;
    }

    painted
}

/// «Портрет» с рамками `BGRSCRNL`/`BGRSCRNR` — общий для главного меню и
/// `Multiplayer Menu` (`menu_draw_menu_portrait`).
fn draw_menu_portrait(frame: &mut RgbaBuffer, menu: &MenuAssets) -> u32 {
    let palette = &menu.palette;
    let mut painted = 0;

    if let Some(ref frame_left) = menu.portrait_frame_left {
        painted += blit_simple(
            frame,
            palette,
            frame_left,
            MAIN_PORTRAIT_LEFT.0,
            MAIN_PORTRAIT_LEFT.1,
            false,
        );
    }
    if let Some(ref frame_right) = menu.portrait_frame_right {
        painted += blit_simple(
            frame,
            palette,
            frame_right,
            MAIN_PORTRAIT_RIGHT.0,
            MAIN_PORTRAIT_RIGHT.1,
            false,
        );
    }
    if let Some(ref portrait) = menu.portrait {
        painted += draw_big_image_at(
            frame,
            portrait,
            palette,
            (MAIN_PORTRAIT.ulx, MAIN_PORTRAIT.uly),
            MAIN_PORTRAIT,
        );
    }

    painted
}

/// Рисует `Multiplayer Menu` (`menu_multiplayer_menu_loop`): `MAINPIC`, заголовок
/// `_(f2b3)`, портрет и шесть кнопок (`Host/Join/Hot Seat/…/Cancel`).
pub fn draw_multiplayer_menu(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, &menu.mainpic);

    draw_title(frame, palette, MULTIPLAYER_TITLE, "Multiplayer Menu");
    painted += draw_menu_portrait(frame, menu);

    for button in &MULTIPLAYER_BUTTONS {
        let pair = if button.big {
            &menu.blank
        } else {
            &menu.small_blank
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));

        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw(
            frame,
            MAIN_STATUS.ulx,
            MAIN_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(BLACK),
        );
    }

    painted
}

/// Рисует главное меню (`menu.cpp::main_menu`).
pub fn draw_main_menu(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, &menu.mainpic);

    draw_title(frame, palette, MAIN_TITLE, "Main Menu");
    painted += draw_menu_portrait(frame, menu);

    draw_text_gnw_centered(
        frame,
        COPYRIGHT,
        GnwFontId::Font5.font(),
        "Copyright 1996 Interplay Productions. v1.04  (M.A.X. Rust client)",
        palette.rgba(BLACK),
    );

    for button in &MAIN_MENU_BUTTONS {
        let pair = if button.big {
            &menu.blank
        } else {
            &menu.small_blank
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));

        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw(
            frame,
            MAIN_STATUS.ulx,
            MAIN_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(BLACK),
        );
    }

    painted
}

/// Рисует экран создания игры/выбора планеты (`planetselectmenu.cpp` +
/// сетка 8×3 Rust-клиента).
pub fn draw_create_form(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    assets: Option<&Assets>,
    form: &CreateForm,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, menu.planet_background());

    draw_title(frame, palette, PLANET_TITLE, "Planet Select Menu");
    let selected_planet = planet_for_world(form.world);
    draw_title(frame, palette, PLANET_NAME, planet_name(selected_planet));

    // Большая картинка выбранной планеты — ровно как
    // `WindowManager_LoadBigImage(SNOW_PIC + world/6, ..., ulx, uly)`:
    // `SNOW_PIC`/`CRTR_PIC`/`GREN_PIC`/`DSRT_PIC` имеют `ulx=26, uly=98`.
    if let Some(picture) = assets.and_then(|a| a.planet_picture(selected_planet)) {
        let picture_rect = PixelRect::new(26, 98, 128, 128);
        painted += draw_big_image_at(frame, picture, palette, (26, 98), picture_rect);
    }

    // Сетка 8×3 внутри области 3×2 слотов PLANETSE.
    let name_font = GnwFontId::Font5.font();

    for index in 0..PLANET_COUNT {
        let cell = planet_cell(index as i32);
        let size = (cell.width - 6).min(cell.height - 24).max(1);
        let content_height = size + 2 + name_font.height();
        let top = cell.uly + ((cell.height - content_height) / 2).max(0);
        let image_rect = PixelRect::new(cell.ulx + (cell.width - size) / 2, top, size, size);
        let planet = crate::ui::planet_from_index(index);
        let picture = planet.and_then(|planet| assets.and_then(|a| a.planet_picture(planet)));
        let selected = form.world == index as i64;

        // Тёмная подложка ячейки: сетка 8×3 ложится на 3×2 чёрных слота
        // `PLANETSE` ровным «полотном», а не рваными кусками фона.
        frame.fill_rect(cell, palette.rgba(BLACK));

        if let (Some(picture), Some(planet)) = (picture, planet) {
            painted += blit_big_scaled(frame, picture, image_rect, planet_picture_crop(planet));
        }

        if let Some(planet) = planet {
            let max_width = (cell.width - 2).max(1);
            let mut name = planet_name(planet).to_string();

            while name_font.text_width(&name) > max_width && name.pop().is_some() {}

            let x = cell.ulx + (cell.width - name_font.text_width(&name)).max(0) / 2;
            let y = top + size + 2;
            let color = if selected { GREEN } else { TEXT };

            draw_text_gnw(frame, x, y, name_font, &name, palette.rgba(color));
        }

        let border = if selected {
            Rgba::rgb(252, 0, 0)
        } else {
            palette.rgba(TEXT).darken(3)
        };
        draw_border(frame, cell, border, if selected { 2 } else { 1 });
    }

    // Поля формы внутри описания PLANETSE.
    for (field, rect) in CREATE_FIELD_RECTS {
        let focused = form.focus == field;
        let mut line = format!("{}: {}", field.label(), form.value(field));

        if focused {
            line.push('_');
        }

        let text = truncate_to_width(line.as_str(), rect.width - 4, GnwFontId::Font5.font());
        let color = if focused { GREEN } else { TEXT };

        draw_text_gnw(
            frame,
            rect.ulx + 2,
            rect.uly + (rect.height - GnwFontId::Font5.font().height()).max(0) / 2,
            GnwFontId::Font5.font(),
            &text,
            palette.rgba(color),
        );
    }

    for button in &PLANET_BUTTONS {
        let Some(pair) = menu.mnubtn_pair(button.resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw_shadowed(
            frame,
            PLANET_STATUS.ulx,
            PLANET_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(GREEN),
            palette.rgba(0xA7),
            palette.rgba(0xE6),
        );
    }

    painted
}

/// Рисует список игр (`networkmenu.cpp::DrawJoinScreen`).
pub fn draw_join_list(
    frame: &mut RgbaBuffer,
    menu: &MenuAssets,
    games: &[max_net::GameInfo],
    selected: usize,
    status: &str,
    mouse: Option<(i32, i32)>,
) -> u32 {
    let palette = &menu.palette;
    let mut painted = draw_big_image(frame, menu.join_background());

    // Рамка списка: чёрная панель с бежевой обводкой (на `MULTGAME` строки
    // хостов лежат на тёмном окне; панель делает их читаемыми и на светлой
    // части фона).
    frame.fill_rect(JOIN_PANEL, Rgba::BLACK);
    draw_border(frame, JOIN_PANEL, palette.rgba(TEXT), 1);
    // Заголовок — на чёрной плашке: на бежевой части `MULTGAME` зелёный
    // GNW-текст почти не читается.
    frame.fill_rect(
        PixelRect::new(
            JOIN_TITLE.ulx,
            JOIN_TITLE.uly - 2,
            JOIN_TITLE.width,
            JOIN_TITLE.height + 4,
        ),
        Rgba::BLACK,
    );
    draw_title(frame, palette, JOIN_TITLE, "JOIN GAME");

    if let Some(ref btnhide) = menu.btnhide {
        let hovered = mouse.is_some_and(|(x, y)| JOIN_HIDE.contains(x, y));
        painted += blit_simple(frame, palette, btnhide, JOIN_HIDE.ulx, JOIN_HIDE.uly, true);
        if hovered {
            // Подсветка рамки `BTNHIDE`: C++ ресурс не использует, поведение
            // (скрыть/назад) — наше.
            draw_border(frame, JOIN_HIDE, palette.rgba(GREEN), 1);
        }
    }

    let scroll = join_scroll(selected);

    if games.is_empty() {
        draw_text_gnw(
            frame,
            JOIN_LIST.ulx + 5,
            JOIN_LIST.uly + 6,
            GnwFontId::Font5.font(),
            "(no games — press Options to refresh)",
            palette.rgba(TEXT),
        );
    }

    for row in 0..JOIN_VISIBLE_ROWS {
        let index = scroll + row;
        let Some(game) = games.get(index) else {
            break;
        };
        let rect = join_row_rect(row);
        let lock = if game.password_protected { " *" } else { "" };
        let line = format!(
            "{}  [{}/{}]{}",
            game.name, game.players, game.min_players, lock
        );
        let text = truncate_to_width(&line, JOIN_LIST.width - 14, GnwFontId::Font5.font());
        let y = rect.uly + (JOIN_ROW_HEIGHT - GnwFontId::Font5.font().height()).max(0) / 2;

        if index == selected {
            draw_text_gnw(
                frame,
                JOIN_LIST.ulx + 1,
                y,
                GnwFontId::Font5.font(),
                ">",
                palette.rgba(GREEN),
            );
        }

        draw_text_gnw(
            frame,
            JOIN_LIST.ulx + 6,
            y,
            GnwFontId::Font5.font(),
            &text,
            palette.rgba(GREEN),
        );

        // Разделитель строк — `draw_line(..., COLOR_GREEN)` как в C++.
        if row + 1 < JOIN_VISIBLE_ROWS {
            let line_y = rect.uly + JOIN_ROW_HEIGHT - 1;
            for x in JOIN_LIST.ulx..JOIN_LIST.ulx + JOIN_LIST.width {
                frame.set(x, line_y, palette.rgba(GREEN));
            }
        }

        if index == selected {
            let line_y = rect.uly + JOIN_ROW_HEIGHT - 2;
            for x in JOIN_LIST.ulx..JOIN_LIST.ulx + JOIN_LIST.width {
                frame.set(x, line_y, palette.rgba(GREEN));
            }
        }
    }

    for button in &JOIN_BUTTONS {
        let Some(pair) = menu.mnubtn_pair(button.resource) else {
            continue;
        };
        let hovered = mouse.is_some_and(|(x, y)| button.rect.contains(x, y));
        painted += draw_image_button(
            frame,
            palette,
            pair.image(false),
            pair.image(true),
            button.rect,
            button.label,
            hovered,
        );
    }

    if !status.is_empty() {
        let status = clean_status(status);
        draw_text_gnw_shadowed(
            frame,
            JOIN_STATUS.ulx,
            JOIN_STATUS.uly,
            GnwFontId::Font5.font(),
            &status,
            palette.rgba(GREEN),
            palette.rgba(0xA7),
            palette.rgba(0xE6),
        );
    }

    painted
}

/// Кнопка главного меню под точкой.
pub fn main_menu_at(x: i32, y: i32) -> Option<MainMenuAction> {
    MAIN_MENU_BUTTONS
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| button.action)
}

/// Прямоугольник кнопки главного меню по действию.
pub fn main_menu_rect(action: MainMenuAction) -> Option<PixelRect> {
    MAIN_MENU_BUTTONS
        .iter()
        .find(|button| button.action == action)
        .map(|button| button.rect)
}

/// Кнопка нижней строки формы под точкой.
pub fn create_button_at(x: i32, y: i32) -> Option<ActionButton> {
    PLANET_BUTTONS
        .iter()
        .copied()
        .find(|b| b.rect.contains(x, y))
}

/// Поле формы под точкой.
pub fn create_field_at(x: i32, y: i32) -> Option<CreateField> {
    CREATE_FIELD_RECTS
        .iter()
        .find(|(_, rect)| rect.contains(x, y))
        .map(|(field, _)| *field)
}

/// Кнопка списка игр под точкой.
pub fn join_button_at(x: i32, y: i32) -> Option<JoinButton> {
    if JOIN_HIDE.contains(x, y) {
        return Some(JoinButton::Hide);
    }

    JOIN_BUTTONS
        .iter()
        .find(|button| button.rect.contains(x, y))
        .map(|button| match button.action {
            MenuButtonAction::Random => JoinButton::Refresh,
            MenuButtonAction::Cancel => JoinButton::Back,
            MenuButtonAction::Help => JoinButton::Help,
            MenuButtonAction::Done => JoinButton::Join,
        })
}

/// Сетка планет 8×3 в координатах `PLANETSE`.
pub fn planet_cell(index: i32) -> PixelRect {
    let columns = 8;
    let rows = 3;
    let cell_width = PLANET_GRID.width / columns;
    let cell_height = PLANET_GRID.height / rows;

    PixelRect::new(
        PLANET_GRID.ulx + (index % columns) * cell_width,
        PLANET_GRID.uly + (index / columns) * cell_height,
        cell_width,
        cell_height,
    )
}

/// Планета по индексу `0..24` (`Planet::try_from`).
fn planet_for_world(world: i64) -> max_core::enums::Planet {
    crate::ui::planet_from_index(world.clamp(0, (PLANET_COUNT - 1) as i64) as usize)
        .unwrap_or(max_core::enums::Planet::Snowcrab)
}

/// Обрезает строку по ширине в пикселях (шрифт GNW).
pub fn truncate_to_width(text: &str, max_width: i32, font: &max_render::GnwFont) -> String {
    let mut out = text.to_string();

    while font.text_width(&out) > max_width && out.pop().is_some() {}

    out
}

/// Рамка `thickness` внутри прямоугольника.
fn draw_border(frame: &mut RgbaBuffer, rect: PixelRect, color: Rgba, thickness: i32) {
    for inset in 0..thickness.max(1) {
        let x = rect.ulx + inset;
        let y = rect.uly + inset;
        let width = rect.width - inset * 2;
        let height = rect.height - inset * 2;

        if width <= 0 || height <= 0 {
            break;
        }

        for dx in 0..width {
            frame.set(x + dx, y, color);
            frame.set(x + dx, y + height - 1, color);
        }
        for dy in 0..height {
            frame.set(x, y + dy, color);
            frame.set(x + width - 1, y + dy, color);
        }
    }
}

/// Рисует big image в точку `origin`, обрезая по `clip`, **палитрой окна**.
///
/// C++ `WindowManager_LoadBigImage(..., palette_from_image=false)` не меняет
/// системную палитру: индексы `P_*` интерпретируются палитрой `MAINPIC`. У
/// `P_TANK` собственная палитра отличается (индекс 19 — маджента), поэтому
/// рисовать портрет его палитрой нельзя.
/// Рисует big image палитрой окна в точку `origin` (с обрезкой по `clip`).
///
/// Так C++ рисует картинки без собственной палитры: `palette_from_image =
/// false` → индексы берутся из палитры окна (`WindowManager_LoadBigImage`).
/// Нужно и меню (`PLANETSE`, портрет), и окну перехода хода M-HS (`HELPFRAM`).
pub fn draw_big_image_at(
    frame: &mut RgbaBuffer,
    image: &BigImage,
    palette: &UiPalette,
    origin: (i32, i32),
    clip: PixelRect,
) -> u32 {
    let offset_x = origin.0;
    let offset_y = origin.1;
    let mut painted = 0;

    for y in 0..image.height as u32 {
        for x in 0..image.width as u32 {
            let px = offset_x + x as i32;
            let py = offset_y + y as i32;

            if !clip.contains(px, py) {
                continue;
            }

            if let Some(index) = image.pixel(x, y) {
                if frame.set(px, py, palette.rgba(index)) {
                    painted += 1;
                }
            }
        }
    }

    painted
}

/// Масштабирует big image в `rect` (nearest): картинки планет 128×128 в ячейку.
///
/// `crop_top` — сколько верхних строк картинки пропустить: у `SNOW_PIC` и
/// соседних групп первые 2–10 строк — служебная цветовая полоса ресурса, а не
/// планета. В большой картинке (26,98) она остаётся как в оригинале, а в
/// миниатюрах 8×3 только мешает.
fn blit_big_scaled(
    frame: &mut RgbaBuffer,
    image: &BigImage,
    rect: PixelRect,
    crop_top: u32,
) -> u32 {
    if rect.is_empty() || image.width <= 0 || image.height as u32 <= crop_top {
        return 0;
    }

    let source_height = image.height as u32 - crop_top;
    let mut painted = 0;

    for y in 0..rect.height {
        let source_y = crop_top + (y as u32 * source_height / rect.height as u32);

        for x in 0..rect.width {
            let source_x = (x as u32 * image.width as u32 / rect.width as u32) as u32;
            let Some(index) = image.pixel(source_x, source_y) else {
                continue;
            };

            if index == image.transparent {
                continue;
            }

            let [r, g, b] = image.color(index);

            if frame.set(rect.ulx + x, rect.uly + y, Rgba::rgb(r, g, b)) {
                painted += 1;
            }
        }
    }

    painted
}

/// Верхняя служебная полоса картинки группы планет.
///
/// Замер по `MAX.RES`: `SNOW_PIC` — 10 строк, `CRTR_PIC` — 3, `GREN_PIC` — 2,
/// `DSRT_PIC` — 9 (дальше начинается сама планета).
pub fn planet_picture_crop(planet: max_core::enums::Planet) -> u32 {
    match planet as usize / 6 {
        0 => 10,
        1 => 3,
        2 => 2,
        3 => 9,
        _ => 0,
    }
}

/// Прямоугольник сетки планет в терминах `max_render::ui::Rect`.
pub const fn planet_grid_rect() -> Rect {
    Rect::new(
        PLANET_GRID.ulx,
        PLANET_GRID.uly,
        PLANET_GRID.width,
        PLANET_GRID.height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    fn repo_path(relative: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(relative)
    }

    fn test_assets() -> Option<(max_assets::ResArchive, MenuAssets)> {
        let max_res = repo_path("max-original/MAX.RES");

        if !max_res.is_file() {
            return None;
        }

        let archive = max_assets::ResArchive::open(&max_res).ok()?;
        let menu = MenuAssets::load(&archive)?;

        Some((archive, menu))
    }

    #[test]
    fn main_menu_layout_matches_the_cpp_oracle() {
        // `menu.cpp::main_menu`: 6 больших кнопок 200×29 на x=385 с шагом 35,
        // Exit 100×30 в (435,420).
        for (index, button) in MAIN_MENU_BUTTONS.iter().take(6).enumerate() {
            assert_eq!(
                button.rect,
                PixelRect::new(385, 175 + index as i32 * 35, 200, 29)
            );
            assert!(button.big);
        }

        let exit = MAIN_MENU_BUTTONS[6];
        assert_eq!(exit.rect, PixelRect::new(435, 420, 100, 30));
        assert!(!exit.big);
        assert_eq!(exit.action, MainMenuAction::Quit);

        assert_eq!(MAIN_MENU_BUTTONS[0].label, "New Game");
        assert_eq!(MAIN_MENU_BUTTONS[2].label, "Multiplayer Game");
        assert_eq!(MAIN_MENU_BUTTONS[6].label, "Exit");

        // Кнопки нижней строки — `planetselectmenu.hpp` / `networkmenu.hpp`.
        assert_eq!(PLANET_BUTTONS[0].rect, PixelRect::new(243, 438, 109, 40));
        assert_eq!(PLANET_BUTTONS[3].rect, PixelRect::new(514, 438, 109, 40));
        assert_eq!(JOIN_LIST, PixelRect::new(359, 28, 209, 160));
        assert_eq!(JOIN_ROW_HEIGHT, 20);
    }

    #[test]
    fn clan_logos_are_drawn_at_their_anchor() {
        // `WindowManager_DecodeSimpleImage`: `ulx -= image->ulx`, `uly -= image->uly`.
        let Some((_archive, menu)) = test_assets() else {
            return;
        };

        let logo = menu.clan_logos[1].as_ref().expect("CLN1LOGO");
        // CLN1LOGO: 74×57, ulx=37, uly=28 — якорь внутри картинки.
        assert_eq!((logo.width, logo.height), (74, 57));
        assert_eq!((logo.ulx, logo.uly), (37, 28));

        let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(0, 0, 0));
        blit_simple_anchored(&mut frame, &menu.palette, logo, 41, 40, true);

        // Прямоугольник нарисованного: от (41-37, 40-28) = (4, 12) и не шире
        // картинки — иначе (как было до фикса) логотип уезжал за кнопку.
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for y in 0..480 {
            for x in 0..640 {
                if frame.get(x, y) != Some(Rgba::rgb(0, 0, 0)) {
                    xs.push(x);
                    ys.push(y);
                }
            }
        }

        assert!(!xs.is_empty(), "логотип нарисован");
        assert!(xs.iter().min().copied().unwrap() >= 4);
        assert!(ys.iter().min().copied().unwrap() >= 12);
        assert!(
            xs.iter().max().copied().unwrap() <= 4 + 74 - 1,
            "логотип не должен выходить за 74 px от левого края картинки"
        );
        assert!(ys.iter().max().copied().unwrap() <= 12 + 57 - 1);
    }

    #[test]
    fn quest_caption_is_centered_like_cpp() {
        // `Button::SetCaption` + `Text_TextLine` (`button.cpp:433`, `text.cpp:778`):
        //   uly = ((40 - 16) / 2) = 12; ulx = (48 - (Text_GetWidth+1)) / 2.
        let font = GnwFontId::Font1.font();
        let text_width = font.text_width("?") + 1;
        let expected_x = 465 + (48 - text_width).max(0) / 2;
        let expected_y = 438 + (40 - font.height()).max(0) / 2;

        assert_eq!(expected_x, 484, "знак ? в кнопке 48×40");
        assert_eq!(expected_y, 450);

        let mut frame = RgbaBuffer::filled(640, 480, Rgba::rgb(0, 0, 0));
        draw_text_gnw_centered(
            &mut frame,
            PixelRect::new(465, 438, 48, 40),
            font,
            "?",
            Rgba::WHITE,
        );

        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for y in 438..478 {
            for x in 465..513 {
                if frame.get(x, y) == Some(Rgba::WHITE) {
                    xs.push(x);
                    ys.push(y);
                }
            }
        }

        assert_eq!(xs.iter().min().copied(), Some(expected_x));
        // «Чернила» '?' начинаются со второй строки ячейки (ink bbox y0 = 1).
        assert_eq!(ys.iter().min().copied(), Some(expected_y + 1));
        // По горизонтали знак действительно внутри кнопки, а не у края.
        assert!(xs.iter().max().copied().unwrap() < 513);
        assert!(ys.iter().max().copied().unwrap() < 478);
    }

    #[test]
    fn menu_assets_load_all_button_images_from_max_res() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };

        assert_eq!(menu.blank.size(), (200, 29));
        assert_eq!(menu.small_blank.size(), (100, 30));

        for index in 0..6 {
            let pair = &menu.mnubtn[index];
            let expected = if index == 4 { (48, 40) } else { (109, 40) };
            assert_eq!(pair.size(), expected, "MNUBTN{}", index + 1);
        }

        assert_eq!(
            (menu.mainpic.width, menu.mainpic.height),
            (640, 480),
            "MAINPIC — фон главного меню"
        );
        assert_eq!(
            menu.planet_background().width,
            640,
            "PLANETSE или fallback 640×480"
        );
        assert_eq!(menu.join_background().width, 640);
        assert!(menu.portrait.is_some(), "P_* портрет 300×240");
        assert!(menu.btnhide.is_some(), "BTNHIDE 38×21");
        assert!(menu.portrait_frame_left.is_some());
        assert!(menu.portrait_frame_right.is_some());
    }

    #[test]
    fn main_menu_draws_mainpic_title_and_gold_captions() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };
        let mut frame = RgbaBuffer::new(640, 480);

        draw_main_menu(&mut frame, &menu, "", Some((390, 180)));

        // Фон — MAINPIC (0,0 в его палитре).
        assert_eq!(
            frame.get(0, 0),
            menu.mainpic.rgb(0, 0).map(Rgba::from_rgb),
            "фон главного меню — MAINPIC"
        );

        let gold = menu.palette.rgba(165);
        let mut gold_pixels = 0;

        for y in 160..460 {
            for x in 380..600 {
                if frame.get(x, y) == Some(gold) {
                    gold_pixels += 1;
                }
            }
        }

        assert!(
            gold_pixels > 50,
            "подписи кнопок — Fonts_GoldColor: {gold_pixels}"
        );

        // Наведение мыши не нажимает кнопку: кадр с курсором побитово равен
        // кадру без курсора (в оригинале `*D` — только пока кнопка нажата).
        let mut idle = RgbaBuffer::new(640, 480);
        draw_main_menu(&mut idle, &menu, "", None);
        assert_eq!(
            frame.pixels(),
            idle.pixels(),
            "наведение не должно рисовать нажатую кнопку *D"
        );
    }

    #[test]
    fn create_form_draws_planetse_grid_and_mnubtn_buttons() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };
        let mut frame = RgbaBuffer::new(640, 480);
        let form = CreateForm {
            world: 12,
            ..Default::default()
        };

        draw_create_form(&mut frame, &menu, None, &form, "status", None);

        // Фон — PLANETSE (если он есть в MAX.RES).
        if let Some(ref planetse) = menu.planetse {
            assert_eq!(
                frame.get(600, 300),
                planetse.rgb(600, 300).map(Rgba::from_rgb),
                "фон формы — PLANETSE"
            );
        }

        // Кнопка Done — MNUBTN6U в (514,438). Первый пиксель картинки —
        // прозрачный (C++ `Image::Blend`), поэтому берём пиксель рамки (2,2).
        let pair = menu.mnubtn_pair(6).expect("MNUBTN6");
        let source = pair
            .pixel(false, 2, 2)
            .map(|index| menu.palette.rgba(index));

        if let Some(source) = source {
            assert_eq!(frame.get(514 + 2, 438 + 2), Some(source));
        }

        // Выбранная планета обведена красным.
        let cell = planet_cell(12);
        let red = Rgba::rgb(252, 0, 0);
        assert_eq!(frame.get(cell.ulx, cell.uly), Some(red));
    }

    #[test]
    fn join_list_draws_multgame_frame_rows_and_buttons() {
        let Some((_archive, menu)) = test_assets() else {
            return;
        };
        let games = vec![
            max_net::GameInfo {
                id: "g1".to_string(),
                name: "Alpha".to_string(),
                world: 0,
                status: max_net::GameStatus::Running,
                port: 31554,
                players: 1,
                min_players: 2,
                password_protected: false,
            },
            max_net::GameInfo {
                id: "g2".to_string(),
                name: "Beta".to_string(),
                world: 0,
                status: max_net::GameStatus::Running,
                port: 31554,
                players: 2,
                min_players: 2,
                password_protected: true,
            },
        ];
        let mut frame = RgbaBuffer::new(640, 480);

        draw_join_list(&mut frame, &menu, &games, 1, "2 game(s)", None);

        if let Some(ref multgame) = menu.multgame {
            assert_eq!(
                frame.get(10, 10),
                multgame.rgb(10, 10).map(Rgba::from_rgb),
                "фон списка — MULTGAME"
            );
        }

        // Строки — зелёным GNW-шрифтом.
        let green = menu.palette.rgba(GREEN);
        let mut green_pixels = 0;
        for y in JOIN_LIST.uly..JOIN_LIST.uly + JOIN_LIST.height {
            for x in JOIN_LIST.ulx..JOIN_LIST.ulx + JOIN_LIST.width {
                if frame.get(x, y) == Some(green) {
                    green_pixels += 1;
                }
            }
        }
        assert!(
            green_pixels > 100,
            "строки/разделители списка: {green_pixels}"
        );

        // Кнопка Ready — MNUBTN6U (рамка в (2,2), первый пиксель прозрачный).
        let pair = menu.mnubtn_pair(6).expect("MNUBTN6");
        if let Some(source) = pair
            .pixel(false, 2, 2)
            .map(|index| menu.palette.rgba(index))
        {
            assert_eq!(frame.get(514 + 2, 438 + 2), Some(source));
        }

        // BTNHIDE нарисован в правом верхнем углу.
        let hide = menu.btnhide.as_ref().expect("BTNHIDE");
        if let Some(source) = hide.pixel(5, 5).map(|index| menu.palette.rgba(index)) {
            assert_eq!(
                frame.get(JOIN_HIDE.ulx + 5, JOIN_HIDE.uly + 5),
                Some(source)
            );
        }
    }

    #[test]
    fn hit_testing_follows_the_cpp_rectangles() {
        assert_eq!(
            main_menu_at(385 + 100, 175 + 14),
            Some(MainMenuAction::Create)
        );
        assert_eq!(
            main_menu_at(385 + 100, 245 + 14),
            Some(MainMenuAction::Join)
        );
        assert_eq!(main_menu_at(435 + 50, 420 + 15), Some(MainMenuAction::Quit));
        assert_eq!(main_menu_at(100, 100), None);

        assert_eq!(
            create_button_at(243 + 10, 438 + 10).map(|b| b.action),
            Some(MenuButtonAction::Random)
        );
        assert_eq!(
            create_button_at(514 + 10, 438 + 10).map(|b| b.action),
            Some(MenuButtonAction::Done)
        );
        assert_eq!(create_field_at(48 + 2, 352 + 2), Some(CreateField::Name));
        assert_eq!(create_field_at(426 + 2, 378 + 2), Some(CreateField::World));

        assert_eq!(
            join_button_at(243 + 10, 438 + 10),
            Some(JoinButton::Refresh)
        );
        assert_eq!(join_button_at(354 + 10, 438 + 10), Some(JoinButton::Back));
        assert_eq!(join_button_at(514 + 10, 438 + 10), Some(JoinButton::Join));
        assert_eq!(
            join_button_at(JOIN_HIDE.ulx + 2, JOIN_HIDE.uly + 2),
            Some(JoinButton::Hide)
        );

        // Строки: 8 видимых, 20 px.
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 5, 3, 0),
            Some(0)
        );
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 25, 3, 0),
            Some(1)
        );
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 200, 3, 0),
            None
        );
        // Прокрутка: при выборе 9-й строки видимы 2..=9, клик по 0-й строке
        // выбирает 2-ю, по 7-й — 9-ю.
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 5, 10, 9),
            Some(2)
        );
        assert_eq!(
            join_row_at(JOIN_LIST.ulx + 5, JOIN_LIST.uly + 145, 10, 9),
            Some(9)
        );
    }

    #[test]
    fn seven_by_three_planet_grid_fits_the_planetse_slots() {
        assert_eq!(PLANET_GRID, PixelRect::new(192, 51, 388, 249));
        assert_eq!(planet_cell(0), PixelRect::new(192, 51, 48, 83));
        assert_eq!(planet_cell(7), PixelRect::new(528, 51, 48, 83));
        assert_eq!(planet_cell(23), PixelRect::new(528, 217, 48, 83));

        for index in 0..PLANET_COUNT as i32 {
            let cell = planet_cell(index);
            assert!(cell.intersect(PLANET_GRID) == Some(cell));
        }
    }
}
