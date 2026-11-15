//! Фаза мобилизации перед высадкой — минимальный порт `CargoMenu`.
//!
//! # Что это за фаза
//!
//! В C++ `GameManager_PlayerMissionSetup` (`game_manager.cpp:1237`) для каждой
//! человеческой команды сначала считает бюджет
//! `team_gold = start_gold + Clans::GetCredits(clan)`, и если он не ноль —
//! открывает `CargoMenu` (`cargomenu.cpp`, фон `CARGOPIC`): покупка стартовых
//! юнитов за кредиты. Только после этого — выбор точки высадки
//! (`GameManager_SelectSite`) и, в самом конце `GameManager_InitGame`, высадка
//! (`GameManager_ProcessTeamMissionSupplyUnits`).
//!
//! # Что портировано точно
//!
//! * Цена юнита — дословно `CargoMenu::BuyUnit`: `Cargo_GetRawConsumptionRate(LANDPLT, 1)`
//!   (`cargo.cpp:140`, для `LANDPLT` = 3) умножить на `ATTRIB_TURNS`
//!   (`UnitValues::GetAttribute`). Возврат при удалении — та же цена
//!   (`CargoMenu::DeleteUnit`).
//! * Бесплатная загрузка `SC_L0001` (`CONSTRCT` 40, `ENGINEER` 20, `SURVEYOR` 0)
//!   остаётся в списке, но её нельзя удалить (`DeleteUnit` в C++ запрещён для
//!   `index < unit_count`).
//! * `start_gold = 0` → фазы нет (C++ уходит в `GetMissionLoadout` без меню).
//! * Список доступного — ровно `CargoMenu::unit_types1` (15 типов).
//!
//! # Что упрощено (честно)
//!
//! * Полный `CargoMenu` — это `AbstractUpgradeMenu` с покупкой апгрейдов
//!   атрибутов, загрузкой cargo в юниты и полосами прокрутки. Здесь — только
//!   список покупки/удаления юнитов, кредиты и итоговая загрузка. Апгрейды
//!   атрибутов не покупаются (в C++ их цена — `UpgradeControl_CalculateCost`,
//!   `pow(...)` с исследовательскими множителями `ResearchMenu_CalculateFactor`,
//!   которые в Rust ещё не портированы).
//! * Клановые бонусы (Clans::GetCredits == 0 для всех кланов `SC_CLANS`) уже
//!   применяются к атрибутам команд в `App::new_game_on_surface`
//!   (`unit_values::team_base_values`), поэтому отдельного шага им не нужно.
//! * Кадр — `CARGOPIC` + списки и кнопки по координатам C++ (`cargomenu.cpp`):
//!   `Buy` (590,386,41,22), `Delete` (412,240,53,23), `Cancel` (357,452,62,23),
//!   `Done` (447,452,54,23). Строки списков рисуются текстом.

use max_core::deploy::{TeamSupply, DEFAULT_MISSION_LOADOUT};
use max_core::enums::{TeamClan, UnitType};
use max_core::team::TeamId;
use max_core::unit_values::{clan_tradeoff, ATTRIB_TURNS};
use max_render::buffer::{PixelRect, RgbaBuffer};
use max_render::text::{draw_text_gnw, draw_text_gnw_centered, GnwFontId};

use crate::menu::MenuAssets;

/// `Cargo_GetRawConsumptionRate(LANDPLT, 1)` (`cargo.cpp:168`).
pub const LANDPLT_RAW_CONSUMPTION: u32 = 3;

/// Список покупки — `CargoMenu::unit_types1` (`cargomenu.cpp:176`), тот же
/// порядок (он же порядок строк списка в C++).
pub const PURCHASE_UNITS: [UnitType; 15] = [
    UnitType::Constructor,
    UnitType::Engineer,
    UnitType::Surveyor,
    UnitType::Scout,
    UnitType::Scanner,
    UnitType::Tank,
    UnitType::AssaultGun,
    UnitType::RocketLauncher,
    UnitType::MissileCrawler,
    UnitType::MineLayer,
    UnitType::RepairUnit,
    UnitType::SupplyTruck,
    UnitType::FuelTruck,
    UnitType::MobileAntiAir,
    UnitType::GoldTruck,
];

/// Английские имена из `ResourceManager_GetUnit(...).GetSingularName()`.
pub const PURCHASE_NAMES: [&str; 15] = [
    "Constructor",
    "Engineer",
    "Surveyor",
    "Scout",
    "Scanner",
    "Tank",
    "Assault Gun",
    "Rocket Launcher",
    "Missile Crawler",
    "Mine Layer",
    "Repair Unit",
    "Supply Truck",
    "Fuel Truck",
    "Mobile AA",
    "Gold Truck",
];

/// Строки списков (координаты C++ `cargomenu.cpp`).
pub const AVAILABLE_AREA: PixelRect = PixelRect::new(482, 80, 123, 290);
/// Строка списка «доступно».
pub const AVAILABLE_ROW: i32 = 15;
/// Строка списка «куплено».
pub const PURCHASED_ROW: i32 = 15;
/// Верх списка «куплено».
pub const PURCHASED_TOP: i32 = 45;
/// Левая граница списка «куплено».
pub const PURCHASED_LEFT: i32 = 337;
/// Кнопка `Buy` (`CRGBUY_U` 41×22).
pub const BUY_BUTTON: PixelRect = PixelRect::new(590, 386, 41, 22);
/// Кнопка `Delete` (`CRGDEL_U` 53×23).
pub const DELETE_BUTTON: PixelRect = PixelRect::new(412, 240, 53, 23);
/// Кнопка `Cancel` (`BLDCAN_U` 62×23).
pub const CANCEL_BUTTON: PixelRect = PixelRect::new(357, 452, 62, 23);
/// Кнопка `Done` (`BLDONE_U` 54×23).
pub const DONE_BUTTON: PixelRect = PixelRect::new(447, 452, 54, 23);

/// Цена юнита в кредитах — `CargoMenu::BuyUnit`.
///
/// C++ берёт `unitvalues_actual[unit]`, то есть **текущие** значения команды:
/// к базовым уже применены клановые trade-off'ы (`GetCurrentUnitValues`).
pub fn unit_cost(unit: UnitType, clan: TeamClan) -> u32 {
    let turns = max_core::unit_values::team_base_values(clan)[unit as usize]
        .get_attribute(ATTRIB_TURNS)
        .max(0) as u32;

    LANDPLT_RAW_CONSUMPTION * turns
}

/// Имя юнита для списка.
pub fn unit_label(unit: UnitType) -> &'static str {
    PURCHASE_UNITS
        .iter()
        .position(|candidate| *candidate == unit)
        .map(|index| PURCHASE_NAMES[index])
        .unwrap_or("?")
}

/// Строка кланового бонуса (для подсказки на экране мобилизации).
pub fn clan_summary(clan: TeamClan) -> String {
    if clan == TeamClan::Random {
        return "Clan: random".to_string();
    }

    let mut bonuses = Vec::new();

    for unit in [
        UnitType::Scout,
        UnitType::Tank,
        UnitType::Scanner,
        UnitType::Constructor,
        UnitType::Engineer,
    ] {
        let attack = clan_tradeoff(clan, unit, max_core::unit_values::ATTRIB_ATTACK);
        let speed = clan_tradeoff(clan, unit, max_core::unit_values::ATTRIB_SPEED);
        let turns = clan_tradeoff(clan, unit, ATTRIB_TURNS);

        if attack != 0 {
            bonuses.push(format!("{} {:+} atk", unit_label(unit), attack));
        }
        if speed != 0 {
            bonuses.push(format!("{} {:+} spd", unit_label(unit), speed));
        }
        if turns != 0 {
            bonuses.push(format!("{} {:+} turns", unit_label(unit), turns));
        }
    }

    if bonuses.is_empty() {
        format!("Clan: {}", crate::setup::clan_name(clan))
    } else {
        format!("Clan {}: {}", crate::setup::clan_name(clan), bonuses.join(", "))
    }
}

/// Состояние экрана мобилизации одной команды.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mobilization {
    /// Чья мобилизация.
    pub team: TeamId,
    /// Клан команды (для строки бонусов).
    pub clan: TeamClan,
    /// Кредиты на входе (`start_gold`).
    pub credits_start: u32,
    /// Остаток кредитов (`team_gold`).
    pub credits: u32,
    /// Купленные юниты (в порядке покупки).
    pub bought: Vec<TeamSupply>,
    /// Бесплатная загрузка миссии (`SC_L0001`), удалять нельзя.
    pub free: Vec<TeamSupply>,
    /// Выбранная строка списка «доступно».
    pub selected: usize,
    /// Выбранная строка списка «куплено» (для Delete).
    pub selected_bought: Option<usize>,
    /// Строка статуса.
    pub status: String,
}

impl Mobilization {
    /// Мобилизация команды: `team_gold = start_gold + clan credits` (0 у всех
    /// кланов `SC_CLANS`).
    pub fn new(team: TeamId, clan: TeamClan, start_gold: u32) -> Self {
        Self {
            team,
            clan,
            credits_start: start_gold,
            credits: start_gold,
            bought: Vec::new(),
            free: DEFAULT_MISSION_LOADOUT.to_vec(),
            selected: 0,
            selected_bought: None,
            status: String::new(),
        }
    }

    /// Нужна ли фаза вообще: в C++ `CargoMenu` не открывается при нулевом
    /// бюджете (`if (team_gold)` в `GameManager_PlayerMissionSetup`).
    pub fn needed(start_gold: u32) -> bool {
        start_gold > 0
    }

    /// Итоговая загрузка: `TeamMissionSupplies::units` (бесплатная + купленная).
    pub fn loadout(&self) -> Vec<TeamSupply> {
        let mut loadout = self.free.clone();
        loadout.extend(self.bought.iter().copied());
        loadout
    }

    /// Цена текущей строки списка «доступно».
    pub fn selected_cost(&self) -> u32 {
        unit_cost(
            PURCHASE_UNITS[self.selected.min(PURCHASE_UNITS.len() - 1)],
            self.clan,
        )
    }

    /// `CargoMenu::BuyUnit`: купить выбранный юнит, если хватает кредитов.
    pub fn buy_selected(&mut self) -> bool {
        self.buy(self.selected)
    }

    /// Покупка юнита по индексу списка `PURCHASE_UNITS`.
    pub fn buy(&mut self, index: usize) -> bool {
        let Some(&unit) = PURCHASE_UNITS.get(index) else {
            return false;
        };
        let cost = unit_cost(unit, self.clan);

        if cost > self.credits {
            self.status = format!("Not enough credits: {} costs {cost}", unit_label(unit));
            return false;
        }

        self.credits -= cost;
        self.bought.push(TeamSupply {
            unit_type: unit,
            cargo: 0,
        });
        self.selected_bought = Some(self.bought.len() - 1);
        self.status = format!("Bought {} for {cost} credits", unit_label(unit));
        true
    }

    /// `CargoMenu::DeleteUnit`: продать последний купленный юнит.
    ///
    /// В C++ удалять можно только строки после бесплатной загрузки
    /// (`index >= unit_count`), поэтому `free` не трогается никогда.
    pub fn delete_selected(&mut self) -> bool {
        let Some(index) = self.selected_bought else {
            self.status = "Select a purchased unit to delete".to_string();
            return false;
        };

        if index >= self.bought.len() {
            return false;
        }

        let supply = self.bought.remove(index);
        let refund = unit_cost(supply.unit_type, self.clan);

        self.credits = self.credits.saturating_add(refund);
        self.selected_bought = if self.bought.is_empty() {
            None
        } else {
            Some(index.min(self.bought.len() - 1))
        };
        self.status = format!("Sold {} for {refund} credits", unit_label(supply.unit_type));
        true
    }

    /// Стрелка вверх/вниз по списку «доступно».
    pub fn step(&mut self, delta: i32) {
        let count = PURCHASE_UNITS.len() as i32;
        let next = (self.selected as i32 + delta).rem_euclid(count);

        self.selected = next as usize;
    }

    /// Отрисовка экрана (1:1 координаты `cargomenu.cpp`).
    pub fn draw(
        &self,
        frame: &mut RgbaBuffer,
        menu: &MenuAssets,
        status: &str,
        mouse: Option<(i32, i32)>,
    ) -> u32 {
        let palette = &menu.palette;
        let mut painted = match menu.cargopic.as_ref() {
            Some(background) => crate::menu::draw_big_image(frame, background),
            None => {
                frame.fill_rect(
                    PixelRect::new(0, 0, 640, 480),
                    palette.rgba(crate::setup::TEXT),
                );
                0
            }
        };
        let font = GnwFontId::Font5.font();
        let header = palette.rgba(crate::setup::GREEN);

        // Заголовок — по центру чёрной плашки CARGOPIC (C++ `Text_TextBox(…, 473,
        // 7, 158, 18, COLOR_GREEN, true)`).
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(473, 7, 158, 18),
            font,
            "Purchase Menu",
            header,
        )
        .max(0) as u32;
        painted += draw_text_gnw_centered(
            frame,
            PixelRect::new(PURCHASED_LEFT, 22, 118, 10),
            font,
            "Purchased",
            header,
        )
        .max(0) as u32;
        painted += draw_text_gnw(frame, 482, 57, font, "Available", header).max(0) as u32;

        // Список «куплено»: бесплатная загрузка, затем покупки.
        for (row, supply) in self.free.iter().chain(self.bought.iter()).enumerate() {
            let y = PURCHASED_TOP + row as i32 * PURCHASED_ROW;

            if y + PURCHASED_ROW > 230 {
                break;
            }

            let selected = self
                .selected_bought
                .is_some_and(|index| index + self.free.len() == row);
            let color = if selected {
                palette.rgba(crate::setup::GREEN)
            } else {
                palette.rgba(crate::setup::TEXT)
            };
            let label = format!("{} ({})", unit_label(supply.unit_type), supply.cargo);

            painted += draw_text_gnw(frame, PURCHASED_LEFT + 4, y, font, &label, color).max(0) as u32;
        }

        // Список «доступно»: 15 строк по 15 px. Цена — сразу за именем
        // (`ReportStats_DrawNumber` в C++ рисует её у правого края строки, но
        // окно списка 123 px, поэтому длинные имена упираются в цену).
        for (row, unit) in PURCHASE_UNITS.iter().enumerate() {
            let y = AVAILABLE_AREA.uly + row as i32 * AVAILABLE_ROW;
            let color = if row == self.selected {
                palette.rgba(crate::setup::GREEN)
            } else {
                palette.rgba(crate::setup::TEXT)
            };
            let price = unit_cost(*unit, self.clan).to_string();
            let price_width = font.text_width(&price);
            let max_label = (AVAILABLE_AREA.width - price_width - 8).max(12);
            let label = fit_label(unit_label(*unit), max_label, font);
            let name_width = font.text_width(&label);

            painted +=
                draw_text_gnw(frame, AVAILABLE_AREA.ulx, y, font, &label, color).max(0) as u32;

            let price_x = AVAILABLE_AREA.ulx + name_width + 6;

            painted +=
                draw_text_gnw(frame, price_x, y, font, &price, palette.rgba(crate::setup::YELLOW.0))
                    .max(0) as u32;
        }

        // Сводка под списками — координаты C++ (`_(ab7e)` Cost, `_(3201)` Credit,
        // `_(fc09)` Cargo: 320/358/409, 283, ширина 48, центрирование).
        let cargo: u16 = self.loadout().iter().map(|supply| supply.cargo).sum();
        for (x, label, value) in [
            (320, "Cost", self.selected_cost()),
            (358, "Credit", self.credits),
            (409, "Cargo", u32::from(cargo)),
        ] {
            painted += draw_text_gnw_centered(
                frame,
                PixelRect::new(x, 283, 48, 16),
                font,
                label,
                palette.rgba(crate::setup::TEXT),
            )
            .max(0) as u32;
            painted += draw_text_gnw_centered(
                frame,
                PixelRect::new(x, 267, 48, 14),
                font,
                &value.to_string(),
                palette.rgba(crate::setup::YELLOW.0),
            )
            .max(0) as u32;
        }

        let clan_line = clan_summary(self.clan);
        painted += draw_text_gnw(
            frame,
            11,
            466,
            font,
            &clan_line,
            palette.rgba(crate::setup::GREEN),
        )
        .max(0) as u32;

        let line = if status.is_empty() {
            self.status.as_str()
        } else {
            status
        };
        painted += draw_text_gnw(
            frame,
            11,
            451,
            font,
            &line.chars().take(44).collect::<String>(),
            palette.rgba(crate::setup::TEXT),
        )
        .max(0) as u32;

        // Кнопки: Buy/Delete/Cancel/Done (картинки C++).
        for (pair, rect, label) in [
            (menu.cargo_buy.as_ref(), BUY_BUTTON, "Buy"),
            (menu.cargo_delete.as_ref(), DELETE_BUTTON, "Delete"),
            (menu.cargo_cancel.as_ref(), CANCEL_BUTTON, "Cancel"),
            (menu.cargo_done.as_ref(), DONE_BUTTON, "Done"),
        ] {
            if let Some(pair) = pair {
                let hovered = mouse.is_some_and(|(x, y)| rect.contains(x, y));
                painted += crate::menu::draw_image_button(
                    frame,
                    palette,
                    pair.image(false),
                    pair.image(true),
                    rect,
                    label,
                    hovered,
                );
            } else {
                // Без ресурса — честная рамка с подписью.
                frame.fill_rect(rect, palette.rgba(crate::setup::TEXT));
                draw_text_gnw_centered(
                    frame,
                    rect,
                    GnwFontId::Font1.font(),
                    label,
                    palette.rgba(crate::setup::GREEN),
                );
                painted += 1;
            }
        }

        painted
    }
}

/// Обрезает имя по ширине, добавляя `..` (чтобы «Rocket Launcher» не выглядел
/// как «Rocket Launch», а честно читался как сокращение).
fn fit_label(text: &str, max_width: i32, font: &max_render::text::GnwFont) -> String {
    if font.text_width(text) <= max_width {
        return text.to_string();
    }

    let mut out = text.to_string();

    while !out.is_empty() && font.text_width(&format!("{out}..")) > max_width {
        out.pop();
    }

    format!("{out}..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purchase_costs_match_the_cpp_formula() {
        // `CargoMenu::BuyUnit`: 3 * ATTRIB_TURNS (`cargo.cpp:168`), таблица
        // `SC_ATTRI`: CONSTRCT 8, ENGINEER 6, TANK 4, SURVEYOR 3.
        assert_eq!(unit_cost(UnitType::Constructor, TeamClan::Random), 24);
        assert_eq!(unit_cost(UnitType::Engineer, TeamClan::Random), 18);
        assert_eq!(unit_cost(UnitType::Tank, TeamClan::Random), 12);
        assert_eq!(unit_cost(UnitType::Surveyor, TeamClan::Random), 9);
        assert_eq!(PURCHASE_UNITS.len(), 15, "CargoMenu::unit_types1");
        assert_eq!(LANDPLT_RAW_CONSUMPTION, 3);
    }

    #[test]
    fn clan_tradeoffs_change_the_price() {
        // Musashi: TANK «Turns to build» −? — цена считается по текущим
        // значениям команды, поэтому обязана совпадать с `team_base_values`.
        let base = unit_cost(UnitType::Tank, TeamClan::Random);
        let musashi = unit_cost(UnitType::Tank, TeamClan::Musashi);

        assert_eq!(
            musashi,
            LANDPLT_RAW_CONSUMPTION
                * max_core::unit_values::team_base_values(TeamClan::Musashi)[UnitType::Tank as usize]
                    .get_attribute(ATTRIB_TURNS)
                    .max(0) as u32
        );
        assert!(base > 0);
    }

    #[test]
    fn buying_and_selling_conserves_credits() {
        let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);

        assert!(mobilization.buy(0), "Constructor за 24 кредита");
        assert_eq!(mobilization.credits, 126);
        assert_eq!(mobilization.bought.len(), 1);

        assert!(mobilization.delete_selected());
        assert_eq!(mobilization.credits, 150);
        assert!(mobilization.bought.is_empty());
        assert_eq!(
            mobilization.loadout().len(),
            DEFAULT_MISSION_LOADOUT.len(),
            "бесплатная загрузка остаётся"
        );
    }

    #[test]
    fn purchase_is_rejected_without_credits() {
        let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 10);

        assert!(!mobilization.buy(0));
        assert!(mobilization.bought.is_empty());
        assert_eq!(mobilization.credits, 10);
        assert!(mobilization.status.contains("Not enough credits"));
    }

    #[test]
    fn delete_never_removes_the_free_loadout() {
        let mut mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);

        mobilization.selected_bought = Some(0);
        assert!(!mobilization.delete_selected(), "удалять нечего");
        assert_eq!(mobilization.loadout(), DEFAULT_MISSION_LOADOUT.to_vec());
    }

    #[test]
    fn zero_budget_skips_the_phase() {
        assert!(!Mobilization::needed(0));
        assert!(Mobilization::needed(150));
    }

    #[test]
    fn long_names_are_shortened_with_dots() {
        let font = GnwFontId::Font5.font();

        assert_eq!(fit_label("Tank", 100, font), "Tank");
        let short = fit_label("Rocket Launcher", 80, font);

        assert!(short.ends_with(".."), "{short}");
        assert!(font.text_width(&short) <= 80, "{short}");

        let mobilization = Mobilization::new(TeamId::RED, TeamClan::Random, 150);
        assert_eq!(mobilization.selected_cost(), unit_cost(UnitType::Constructor, TeamClan::Random));
    }
}
