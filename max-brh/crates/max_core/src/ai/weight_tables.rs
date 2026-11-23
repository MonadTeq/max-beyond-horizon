//! The 18 AI weight tables — filling and adjustment.
//!
//! Oracle: `aiplayer.cpp:70-97` (`RebuildWeightTable`/`RebuildWeightTables`),
//! `aiplayer.cpp:99-158` (`UpdateWeightTables`) and `aiplayer.cpp:2951-3305`
//! (the weight-table block of `AiPlayer::Init`).
//!
//! The tables themselves are fields of [`AiPlayer`] (`aiplayer.hpp:93-110`);
//! this module only fills and adjusts them. They are not consulted by the
//! decision logic yet (phase A.2+), so the tables do not change any AI decision.
//!
//! `WeightTable::Add` silently skips a type the team cannot build
//! (`Builder_IsBuildable`). Buildability is mission data: the default rules
//! (`default.game_rules.lua`) define `max_is_buildable` as "the type appears in
//! at least one builder capability list" — [`crate::production::BUILDABLE`].
//! Per-mission rule overrides are not modeled in max_core yet.

use crate::enums::{AiStrategy, Opponent, UnitType};

use super::strategy::AiPlayer;
use super::weight::WeightTable;

/// `Builder_IsBuildable` (`builder.cpp:31`) under the default game rules.
///
/// `max_is_buildable` (`default.game_rules.lua`) scans
/// `max_builder_capability_list`; the type is buildable when any builder can
/// build it. The `team` argument is not used by the default rules.
fn builder_is_buildable(unit_type: UnitType) -> bool {
    crate::production::BUILDABLE
        .iter()
        .any(|(_, units)| units.contains(&unit_type))
}

/// `WeightTable::Add` (`weighttable.cpp:55`) — adds only when buildable.
fn add(table: &mut WeightTable, unit_type: UnitType, weight: u16) {
    table.add_buildable(unit_type, weight, builder_is_buildable(unit_type));
}

/// `WeightTable::Add` behind an `if (opponent >= OPPONENT_TYPE_*)` guard.
fn add_if(table: &mut WeightTable, condition: bool, unit_type: UnitType, weight: u16) {
    if condition {
        add(table, unit_type, weight);
    }
}

impl AiPlayer {
    /// `AiPlayer::Init` weight-table block (`aiplayer.cpp:2951-3305`).
    ///
    /// `opponent` is `ResourceManager_GetSettings()->GetNumericValue("opponent")`
    /// (`Opponent`). The entry order is the C++ `Add` order, which matters for
    /// `WeightTable::RollUnitType` and `operator+=`.
    ///
    /// `JUGGRNT`/`ALNTANK`/`ALNASGUN`/`ALNPLANE` are added by the oracle but are
    /// not buildable under the default rules, so `Add` drops them — as in C++.
    pub fn init_weight_tables(&mut self, opponent: u8) {
        let team = self.player_team.0;
        let expert = opponent >= Opponent::Expert as u8;
        let average = opponent >= Opponent::Average as u8;
        let apprentice = opponent >= Opponent::Apprentice as u8;

        // `weight_table_ground_defense` (`aiplayer.cpp:2953-2970`).
        self.weight_table_ground_defense.clear();
        self.weight_table_ground_defense.set_team(team);
        add_if(
            &mut self.weight_table_ground_defense,
            expert,
            UnitType::Infiltrator,
            2,
        );
        add(&mut self.weight_table_ground_defense, UnitType::Tank, 1);
        add(
            &mut self.weight_table_ground_defense,
            UnitType::AssaultGun,
            1,
        );
        add(
            &mut self.weight_table_ground_defense,
            UnitType::RocketLauncher,
            1,
        );
        add(
            &mut self.weight_table_ground_defense,
            UnitType::MissileCrawler,
            2,
        );
        add(&mut self.weight_table_ground_defense, UnitType::Gunboat, 1);
        add(
            &mut self.weight_table_ground_defense,
            UnitType::MissileCruiser,
            2,
        );
        add(&mut self.weight_table_ground_defense, UnitType::Bomber, 2);
        add(
            &mut self.weight_table_ground_defense,
            UnitType::Juggernaut,
            1,
        );
        add(
            &mut self.weight_table_ground_defense,
            UnitType::AlienTank,
            1,
        );
        add(
            &mut self.weight_table_ground_defense,
            UnitType::AlienAssaultGun,
            1,
        );
        add(
            &mut self.weight_table_ground_defense,
            UnitType::AlienPlane,
            1,
        );

        // `weight_table_scout` (`aiplayer.cpp:2972-3003`).
        self.weight_table_scout.clear();
        self.weight_table_scout.set_team(team);
        add_if(
            &mut self.weight_table_scout,
            expert,
            UnitType::Infiltrator,
            1,
        );
        add_if(
            &mut self.weight_table_scout,
            average,
            UnitType::Artillery,
            1,
        );
        add_if(
            &mut self.weight_table_scout,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add_if(
            &mut self.weight_table_scout,
            apprentice,
            UnitType::GunTurret,
            1,
        );
        add_if(
            &mut self.weight_table_scout,
            average,
            UnitType::Submarine,
            1,
        );
        add(&mut self.weight_table_scout, UnitType::Scout, 2);
        add(&mut self.weight_table_scout, UnitType::Tank, 1);
        add(&mut self.weight_table_scout, UnitType::AssaultGun, 2);
        add(&mut self.weight_table_scout, UnitType::RocketLauncher, 1);
        add(&mut self.weight_table_scout, UnitType::MissileCrawler, 2);
        add(&mut self.weight_table_scout, UnitType::Gunboat, 2);
        add(&mut self.weight_table_scout, UnitType::MissileCruiser, 2);
        add(&mut self.weight_table_scout, UnitType::Bomber, 3);
        add(&mut self.weight_table_scout, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_scout, UnitType::AlienTank, 1);
        add(&mut self.weight_table_scout, UnitType::AlienAssaultGun, 1);
        add(&mut self.weight_table_scout, UnitType::AlienPlane, 1);

        // `weight_table_tanks` (`aiplayer.cpp:3005-3030`).
        self.weight_table_tanks.clear();
        self.weight_table_tanks.set_team(team);
        add_if(
            &mut self.weight_table_tanks,
            apprentice,
            UnitType::GunTurret,
            1,
        );
        add_if(
            &mut self.weight_table_tanks,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add_if(
            &mut self.weight_table_tanks,
            expert,
            UnitType::Infiltrator,
            1,
        );
        add(&mut self.weight_table_tanks, UnitType::Tank, 2);
        add(&mut self.weight_table_tanks, UnitType::AssaultGun, 1);
        add(&mut self.weight_table_tanks, UnitType::RocketLauncher, 1);
        add(&mut self.weight_table_tanks, UnitType::MissileCrawler, 2);
        add(&mut self.weight_table_tanks, UnitType::Gunboat, 2);
        add(&mut self.weight_table_tanks, UnitType::MissileCruiser, 2);
        add(&mut self.weight_table_tanks, UnitType::Bomber, 3);
        add(&mut self.weight_table_tanks, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_tanks, UnitType::AlienTank, 1);
        add(&mut self.weight_table_tanks, UnitType::AlienAssaultGun, 1);
        add(&mut self.weight_table_tanks, UnitType::AlienPlane, 1);

        // `weight_table_assault_guns` (`aiplayer.cpp:3032-3057`).
        self.weight_table_assault_guns.clear();
        self.weight_table_assault_guns.set_team(team);
        add_if(
            &mut self.weight_table_assault_guns,
            expert,
            UnitType::Infiltrator,
            1,
        );
        add_if(
            &mut self.weight_table_assault_guns,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add_if(
            &mut self.weight_table_assault_guns,
            apprentice,
            UnitType::GunTurret,
            1,
        );
        add(&mut self.weight_table_assault_guns, UnitType::Tank, 2);
        add(&mut self.weight_table_assault_guns, UnitType::AssaultGun, 1);
        add(
            &mut self.weight_table_assault_guns,
            UnitType::RocketLauncher,
            1,
        );
        add(
            &mut self.weight_table_assault_guns,
            UnitType::MissileCrawler,
            2,
        );
        add(&mut self.weight_table_assault_guns, UnitType::Gunboat, 2);
        add(
            &mut self.weight_table_assault_guns,
            UnitType::MissileCruiser,
            2,
        );
        add(&mut self.weight_table_assault_guns, UnitType::Bomber, 3);
        add(&mut self.weight_table_assault_guns, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_assault_guns, UnitType::AlienTank, 1);
        add(
            &mut self.weight_table_assault_guns,
            UnitType::AlienAssaultGun,
            1,
        );
        add(&mut self.weight_table_assault_guns, UnitType::AlienPlane, 1);

        // `weight_table_missile_launcher` (`aiplayer.cpp:3059-3080`).
        self.weight_table_missile_launcher.clear();
        self.weight_table_missile_launcher.set_team(team);
        add_if(
            &mut self.weight_table_missile_launcher,
            expert,
            UnitType::Infiltrator,
            1,
        );
        add_if(
            &mut self.weight_table_missile_launcher,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add(&mut self.weight_table_missile_launcher, UnitType::Tank, 1);
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::AssaultGun,
            1,
        );
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::RocketLauncher,
            1,
        );
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::MissileCrawler,
            2,
        );
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::Gunboat,
            1,
        );
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::MissileCruiser,
            2,
        );
        add(&mut self.weight_table_missile_launcher, UnitType::Bomber, 3);
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::Juggernaut,
            1,
        );
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::AlienTank,
            1,
        );
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::AlienAssaultGun,
            1,
        );
        add(
            &mut self.weight_table_missile_launcher,
            UnitType::AlienPlane,
            1,
        );

        // `weight_table_air_defense` (`aiplayer.cpp:3082-3106`).
        self.weight_table_air_defense.clear();
        self.weight_table_air_defense.set_team(team);
        add_if(
            &mut self.weight_table_air_defense,
            expert,
            UnitType::Infiltrator,
            2,
        );
        add_if(
            &mut self.weight_table_air_defense,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add_if(
            &mut self.weight_table_air_defense,
            apprentice,
            UnitType::GunTurret,
            1,
        );
        add(&mut self.weight_table_air_defense, UnitType::Scout, 1);
        add(&mut self.weight_table_air_defense, UnitType::Tank, 1);
        add(&mut self.weight_table_air_defense, UnitType::AssaultGun, 1);
        add(
            &mut self.weight_table_air_defense,
            UnitType::RocketLauncher,
            1,
        );
        add(
            &mut self.weight_table_air_defense,
            UnitType::MissileCrawler,
            1,
        );
        add(&mut self.weight_table_air_defense, UnitType::Gunboat, 1);
        add(
            &mut self.weight_table_air_defense,
            UnitType::MissileCruiser,
            1,
        );
        add(&mut self.weight_table_air_defense, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_air_defense, UnitType::AlienTank, 1);
        add(
            &mut self.weight_table_air_defense,
            UnitType::AlienAssaultGun,
            1,
        );

        // `weight_table_aircrafts` (`aiplayer.cpp:3108-3118`).
        self.weight_table_aircrafts.clear();
        self.weight_table_aircrafts.set_team(team);
        add_if(
            &mut self.weight_table_aircrafts,
            average,
            UnitType::AntiAir,
            3,
        );
        add(&mut self.weight_table_aircrafts, UnitType::MobileAntiAir, 3);
        add(&mut self.weight_table_aircrafts, UnitType::Escort, 3);
        add(&mut self.weight_table_aircrafts, UnitType::Fighter, 1);
        add(&mut self.weight_table_aircrafts, UnitType::AlienPlane, 1);

        // `weight_table_bomber` (`aiplayer.cpp:3120-3131`).
        self.weight_table_bomber.clear();
        self.weight_table_bomber.set_team(team);
        add_if(&mut self.weight_table_bomber, average, UnitType::AntiAir, 1);
        add(&mut self.weight_table_bomber, UnitType::MobileAntiAir, 1);
        add(&mut self.weight_table_bomber, UnitType::Escort, 1);
        add(&mut self.weight_table_bomber, UnitType::Fighter, 3);
        add(&mut self.weight_table_bomber, UnitType::AlienPlane, 3);

        // `weight_table_9` (`aiplayer.cpp:3133-3143`).
        self.weight_table_9.clear();
        self.weight_table_9.set_team(team);
        add_if(&mut self.weight_table_9, average, UnitType::AntiAir, 1);
        add(&mut self.weight_table_9, UnitType::MobileAntiAir, 1);
        add(&mut self.weight_table_9, UnitType::Escort, 1);
        add(&mut self.weight_table_9, UnitType::Fighter, 1);
        add(&mut self.weight_table_9, UnitType::AlienPlane, 1);

        // `weight_table_fastboat` (`aiplayer.cpp:3145-3162`).
        self.weight_table_fastboat.clear();
        self.weight_table_fastboat.set_team(team);
        add_if(
            &mut self.weight_table_fastboat,
            average,
            UnitType::Artillery,
            1,
        );
        add_if(
            &mut self.weight_table_fastboat,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add_if(
            &mut self.weight_table_fastboat,
            average,
            UnitType::Submarine,
            1,
        );
        add(&mut self.weight_table_fastboat, UnitType::AssaultGun, 1);
        add(&mut self.weight_table_fastboat, UnitType::RocketLauncher, 1);
        add(&mut self.weight_table_fastboat, UnitType::MissileCrawler, 1);
        add(&mut self.weight_table_fastboat, UnitType::Corvette, 1);
        add(&mut self.weight_table_fastboat, UnitType::Gunboat, 1);
        add(&mut self.weight_table_fastboat, UnitType::MissileCruiser, 1);
        add(&mut self.weight_table_fastboat, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_fastboat, UnitType::AlienTank, 1);
        add(
            &mut self.weight_table_fastboat,
            UnitType::AlienAssaultGun,
            1,
        );

        // `weight_table_corvette` (`aiplayer.cpp:3164-3181`).
        self.weight_table_corvette.clear();
        self.weight_table_corvette.set_team(team);
        add_if(
            &mut self.weight_table_corvette,
            average,
            UnitType::Artillery,
            1,
        );
        add_if(
            &mut self.weight_table_corvette,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add(&mut self.weight_table_corvette, UnitType::AssaultGun, 1);
        add(&mut self.weight_table_corvette, UnitType::RocketLauncher, 1);
        add(&mut self.weight_table_corvette, UnitType::MissileCrawler, 1);
        add(&mut self.weight_table_corvette, UnitType::Corvette, 1);
        add(&mut self.weight_table_corvette, UnitType::Gunboat, 2);
        add(&mut self.weight_table_corvette, UnitType::MissileCruiser, 2);
        add(&mut self.weight_table_corvette, UnitType::Bomber, 3);
        add(&mut self.weight_table_corvette, UnitType::Juggernaut, 1);
        add(
            &mut self.weight_table_corvette,
            UnitType::AlienAssaultGun,
            1,
        );
        add(&mut self.weight_table_corvette, UnitType::AlienPlane, 1);

        // `weight_table_submarine` (`aiplayer.cpp:3183-3185`).
        self.weight_table_submarine.clear();
        self.weight_table_submarine.set_team(team);
        add(&mut self.weight_table_submarine, UnitType::Corvette, 1);

        // `weight_table_battleships` (`aiplayer.cpp:3187-3205`).
        self.weight_table_battleships.clear();
        self.weight_table_battleships.set_team(team);
        add_if(
            &mut self.weight_table_battleships,
            average,
            UnitType::Submarine,
            3,
        );
        add_if(
            &mut self.weight_table_battleships,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add(&mut self.weight_table_battleships, UnitType::AssaultGun, 1);
        add(
            &mut self.weight_table_battleships,
            UnitType::RocketLauncher,
            1,
        );
        add(
            &mut self.weight_table_battleships,
            UnitType::MissileCrawler,
            1,
        );
        add(&mut self.weight_table_battleships, UnitType::Corvette, 1);
        add(&mut self.weight_table_battleships, UnitType::Gunboat, 2);
        add(
            &mut self.weight_table_battleships,
            UnitType::MissileCruiser,
            1,
        );
        add(&mut self.weight_table_battleships, UnitType::Bomber, 3);
        add(&mut self.weight_table_battleships, UnitType::Juggernaut, 1);
        add(
            &mut self.weight_table_battleships,
            UnitType::AlienAssaultGun,
            1,
        );
        add(&mut self.weight_table_battleships, UnitType::AlienPlane, 1);

        // `weight_table_support_ships` (`aiplayer.cpp:3207-3233`).
        self.weight_table_support_ships.clear();
        self.weight_table_support_ships.set_team(team);
        add_if(
            &mut self.weight_table_support_ships,
            apprentice,
            UnitType::GunTurret,
            1,
        );
        add_if(
            &mut self.weight_table_support_ships,
            average,
            UnitType::Artillery,
            1,
        );
        add_if(
            &mut self.weight_table_support_ships,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add_if(
            &mut self.weight_table_support_ships,
            average,
            UnitType::Submarine,
            1,
        );
        add(&mut self.weight_table_support_ships, UnitType::Scout, 1);
        add(&mut self.weight_table_support_ships, UnitType::Tank, 1);
        add(
            &mut self.weight_table_support_ships,
            UnitType::AssaultGun,
            1,
        );
        add(
            &mut self.weight_table_support_ships,
            UnitType::RocketLauncher,
            1,
        );
        add(
            &mut self.weight_table_support_ships,
            UnitType::MissileCrawler,
            1,
        );
        add(
            &mut self.weight_table_support_ships,
            UnitType::MobileAntiAir,
            1,
        );
        add(&mut self.weight_table_support_ships, UnitType::Corvette, 1);
        add(&mut self.weight_table_support_ships, UnitType::Gunboat, 1);
        add(
            &mut self.weight_table_support_ships,
            UnitType::MissileCruiser,
            1,
        );
        add(&mut self.weight_table_support_ships, UnitType::Bomber, 1);
        add(
            &mut self.weight_table_support_ships,
            UnitType::Juggernaut,
            1,
        );
        add(&mut self.weight_table_support_ships, UnitType::AlienTank, 1);
        add(
            &mut self.weight_table_support_ships,
            UnitType::AlienAssaultGun,
            1,
        );
        add(
            &mut self.weight_table_support_ships,
            UnitType::AlienPlane,
            1,
        );

        // `weight_table_missileboat` (`aiplayer.cpp:3235-3253`).
        self.weight_table_missileboat.clear();
        self.weight_table_missileboat.set_team(team);
        add_if(
            &mut self.weight_table_missileboat,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add(
            &mut self.weight_table_missileboat,
            UnitType::MissileCrawler,
            1,
        );
        add(&mut self.weight_table_missileboat, UnitType::Corvette, 1);
        add(&mut self.weight_table_missileboat, UnitType::Gunboat, 1);
        add_if(
            &mut self.weight_table_missileboat,
            average,
            UnitType::Submarine,
            3,
        );
        add(
            &mut self.weight_table_missileboat,
            UnitType::MissileCruiser,
            1,
        );
        add(&mut self.weight_table_missileboat, UnitType::Bomber, 3);
        add(&mut self.weight_table_missileboat, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_missileboat, UnitType::AlienPlane, 1);

        // `weight_table_commando` (`aiplayer.cpp:3255-3257`).
        self.weight_table_commando.clear();
        self.weight_table_commando.set_team(team);
        add(&mut self.weight_table_commando, UnitType::Infantry, 1);

        // `weight_table_infantry` (`aiplayer.cpp:3259-3283`).
        self.weight_table_infantry.clear();
        self.weight_table_infantry.set_team(team);
        add_if(
            &mut self.weight_table_infantry,
            average,
            UnitType::MissileLauncher,
            1,
        );
        add_if(
            &mut self.weight_table_infantry,
            apprentice,
            UnitType::GunTurret,
            1,
        );
        add(&mut self.weight_table_infantry, UnitType::Tank, 1);
        add(&mut self.weight_table_infantry, UnitType::AssaultGun, 1);
        add(&mut self.weight_table_infantry, UnitType::RocketLauncher, 1);
        add(&mut self.weight_table_infantry, UnitType::MissileCrawler, 1);
        add(&mut self.weight_table_infantry, UnitType::MobileAntiAir, 1);
        add(&mut self.weight_table_infantry, UnitType::Infantry, 1);
        add(&mut self.weight_table_infantry, UnitType::Gunboat, 1);
        add(&mut self.weight_table_infantry, UnitType::MissileCruiser, 1);
        add(&mut self.weight_table_infantry, UnitType::Bomber, 1);
        add(&mut self.weight_table_infantry, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_infantry, UnitType::AlienTank, 1);
        add(
            &mut self.weight_table_infantry,
            UnitType::AlienAssaultGun,
            1,
        );
        add(&mut self.weight_table_infantry, UnitType::AlienPlane, 1);

        // `weight_table_generic` (`aiplayer.cpp:3285-3304`).
        self.weight_table_generic.clear();
        self.weight_table_generic.set_team(team);
        add_if(
            &mut self.weight_table_generic,
            expert,
            UnitType::Infiltrator,
            1,
        );
        add_if(
            &mut self.weight_table_generic,
            expert,
            UnitType::Infantry,
            1,
        );
        add(&mut self.weight_table_generic, UnitType::Scout, 1);
        add(&mut self.weight_table_generic, UnitType::Tank, 1);
        add(&mut self.weight_table_generic, UnitType::AssaultGun, 1);
        add(&mut self.weight_table_generic, UnitType::RocketLauncher, 1);
        add(&mut self.weight_table_generic, UnitType::MissileCrawler, 1);
        add(&mut self.weight_table_generic, UnitType::Gunboat, 1);
        add(&mut self.weight_table_generic, UnitType::MissileCruiser, 1);
        add(&mut self.weight_table_generic, UnitType::Bomber, 1);
        add(&mut self.weight_table_generic, UnitType::Juggernaut, 1);
        add(&mut self.weight_table_generic, UnitType::AlienTank, 1);
        add(&mut self.weight_table_generic, UnitType::AlienAssaultGun, 1);
        add(&mut self.weight_table_generic, UnitType::AlienPlane, 1);
    }

    /// `AiPlayer::UpdateWeightTables` (`aiplayer.cpp:99-158`).
    ///
    /// Called from `BeginTurn` inside the `need_init` block, after the strategy
    /// is resolved by clan (`aiplayer.cpp:2285`). C++ has no `default:` branch:
    /// `AI_STRATEGY_RANDOM` (and any unhandled value) leaves the tables as is.
    pub fn update_weight_tables(&mut self) {
        match self.strategy {
            AiStrategy::Defensive => {
                self.rebuild_weight_tables(UnitType::GunTurret, 3);
                self.rebuild_weight_tables(UnitType::MissileLauncher, 3);
                self.rebuild_weight_tables(UnitType::AntiAir, 3);
            }
            AiStrategy::Missiles => {
                self.rebuild_weight_tables(UnitType::MissileCrawler, 5);
                self.rebuild_weight_tables(UnitType::MissileCruiser, 5);
            }
            AiStrategy::Air => {
                self.rebuild_weight_tables(UnitType::Fighter, 5);
                self.rebuild_weight_tables(UnitType::Bomber, 5);
            }
            AiStrategy::Sea => {
                self.rebuild_weight_tables(UnitType::Escort, 2);
                self.rebuild_weight_tables(UnitType::Corvette, 2);
                self.rebuild_weight_tables(UnitType::Submarine, 2);
                self.rebuild_weight_tables(UnitType::Gunboat, 2);
                self.rebuild_weight_tables(UnitType::MissileCruiser, 2);
            }
            AiStrategy::ScoutHorde => {
                add(&mut self.weight_table_tanks, UnitType::Scout, 1);
                add(&mut self.weight_table_assault_guns, UnitType::Scout, 2);
                add(&mut self.weight_table_ground_defense, UnitType::Scout, 1);
                add(&mut self.weight_table_missile_launcher, UnitType::Scout, 1);
                self.rebuild_weight_tables(UnitType::Scout, 10);
            }
            AiStrategy::TankHorde => {
                self.rebuild_weight_tables(UnitType::Tank, 10);
            }
            AiStrategy::FastAttack => {
                self.rebuild_weight_tables(UnitType::AssaultGun, 3);
                self.rebuild_weight_tables(UnitType::Bomber, 2);
                self.rebuild_weight_tables(UnitType::MobileAntiAir, 2);
                self.rebuild_weight_tables(UnitType::Corvette, 2);
                self.rebuild_weight_tables(UnitType::Escort, 2);
            }
            AiStrategy::CombinedArms => {
                self.rebuild_weight_tables(UnitType::Tank, 5);
                self.rebuild_weight_tables(UnitType::MissileCrawler, 5);
            }
            AiStrategy::Espionage => {
                self.rebuild_weight_tables(UnitType::Submarine, 4);
                self.rebuild_weight_tables(UnitType::Infiltrator, 4);
                self.rebuild_weight_tables(UnitType::MissileCrawler, 2);
                self.rebuild_weight_tables(UnitType::MissileCruiser, 2);
            }
            AiStrategy::Random => {}
        }
    }

    /// `AiPlayer::RebuildWeightTables` (`aiplayer.cpp:78-97`) — applies
    /// `RebuildWeightTable` (`aiplayer.cpp:70`) to all 18 tables in C++ order.
    pub fn rebuild_weight_tables(&mut self, unit_type: UnitType, factor: u16) {
        self.weight_table_ground_defense.rebuild(unit_type, factor);
        self.weight_table_scout.rebuild(unit_type, factor);
        self.weight_table_tanks.rebuild(unit_type, factor);
        self.weight_table_assault_guns.rebuild(unit_type, factor);
        self.weight_table_missile_launcher
            .rebuild(unit_type, factor);
        self.weight_table_air_defense.rebuild(unit_type, factor);
        self.weight_table_aircrafts.rebuild(unit_type, factor);
        self.weight_table_bomber.rebuild(unit_type, factor);
        self.weight_table_9.rebuild(unit_type, factor);
        self.weight_table_fastboat.rebuild(unit_type, factor);
        self.weight_table_corvette.rebuild(unit_type, factor);
        self.weight_table_submarine.rebuild(unit_type, factor);
        self.weight_table_battleships.rebuild(unit_type, factor);
        self.weight_table_missileboat.rebuild(unit_type, factor);
        self.weight_table_support_ships.rebuild(unit_type, factor);
        self.weight_table_commando.rebuild(unit_type, factor);
        self.weight_table_infantry.rebuild(unit_type, factor);
        self.weight_table_generic.rebuild(unit_type, factor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{Opponent, TeamClan};
    use crate::game::GameState;
    use crate::team::{TeamId, TeamKind};

    /// Entries as `(type, weight)` in insertion order.
    fn entries(table: &WeightTable) -> Vec<(UnitType, u16)> {
        table
            .entries()
            .iter()
            .map(|entry| (entry.unit_type.expect("weight entry"), entry.weight))
            .collect()
    }

    fn player_at(opponent: Opponent) -> AiPlayer {
        let mut player = AiPlayer::new(TeamId::RED);
        player.init_weight_tables(opponent as u8);
        player
    }

    #[test]
    fn init_weight_tables_uses_the_cpp_entry_order_and_weights() {
        let player = player_at(Opponent::Expert);

        // `weight_table_ground_defense`: JUGGRNT/ALN* are not buildable under the
        // default rules, so `Add` drops them (C++ `Builder_IsBuildable`).
        assert_eq!(
            entries(&player.weight_table_ground_defense),
            vec![
                (UnitType::Infiltrator, 2),
                (UnitType::Tank, 1),
                (UnitType::AssaultGun, 1),
                (UnitType::RocketLauncher, 1),
                (UnitType::MissileCrawler, 2),
                (UnitType::Gunboat, 1),
                (UnitType::MissileCruiser, 2),
                (UnitType::Bomber, 2),
            ]
        );

        // `weight_table_submarine` has one entry in C++.
        assert_eq!(
            entries(&player.weight_table_submarine),
            vec![(UnitType::Corvette, 1)]
        );

        // `weight_table_commando`.
        assert_eq!(
            entries(&player.weight_table_commando),
            vec![(UnitType::Infantry, 1)]
        );

        // `weight_table_aircrafts`: ANTIAIR only from AVERAGE on, ALNPLANE dropped.
        assert_eq!(
            entries(&player.weight_table_aircrafts),
            vec![
                (UnitType::AntiAir, 3),
                (UnitType::MobileAntiAir, 3),
                (UnitType::Escort, 3),
                (UnitType::Fighter, 1),
            ]
        );

        // Every table gets the player's team and is non-empty.
        assert_eq!(player.weight_table_ground_defense.team(), TeamId::RED.0);
        assert_eq!(player.weight_table_9.team(), TeamId::RED.0);
        assert!(!player.weight_table_9.is_empty());
        assert!(!player.weight_table_bomber.is_empty());
        assert!(!player.weight_table_fastboat.is_empty());
        assert!(!player.weight_table_corvette.is_empty());
        assert!(!player.weight_table_battleships.is_empty());
        assert!(!player.weight_table_missileboat.is_empty());
        assert!(!player.weight_table_support_ships.is_empty());
        assert!(!player.weight_table_infantry.is_empty());
        assert!(!player.weight_table_generic.is_empty());
        assert!(!player.weight_table_tanks.is_empty());
        assert!(!player.weight_table_scout.is_empty());
        assert!(!player.weight_table_assault_guns.is_empty());
        assert!(!player.weight_table_missile_launcher.is_empty());
        assert!(!player.weight_table_air_defense.is_empty());
        assert!(!player.weight_table_aircrafts.is_empty());
        assert!(!player.weight_table_bomber.is_empty());
        assert!(!player.weight_table_commando.is_empty());
    }

    #[test]
    fn init_weight_tables_follows_opponent_thresholds() {
        // CLUELESS: no threshold-guarded entries at all.
        let clueless = player_at(Opponent::Clueless);
        assert_eq!(
            entries(&clueless.weight_table_scout),
            vec![
                (UnitType::Scout, 2),
                (UnitType::Tank, 1),
                (UnitType::AssaultGun, 2),
                (UnitType::RocketLauncher, 1),
                (UnitType::MissileCrawler, 2),
                (UnitType::Gunboat, 2),
                (UnitType::MissileCruiser, 2),
                (UnitType::Bomber, 3),
            ]
        );
        assert_eq!(
            clueless
                .weight_table_ground_defense
                .get_weight(UnitType::Infiltrator),
            0
        );

        // APPRENTICE: GUNTURRT appears (the guard is `>= APPRENTICE`).
        let apprentice = player_at(Opponent::Apprentice);
        assert_eq!(
            apprentice
                .weight_table_scout
                .get_weight(UnitType::GunTurret),
            1
        );
        assert_eq!(
            apprentice
                .weight_table_scout
                .get_weight(UnitType::Artillery),
            0
        );
        assert_eq!(
            apprentice
                .weight_table_scout
                .get_weight(UnitType::Submarine),
            0
        );

        // AVERAGE: ARTYTRRT/ANTIMSSL (and SUBMARNE) are added after the
        // APPRENTICE block — the C++ order (artillery, missile launcher,
        // gun turret, submarine) is preserved.
        let average = player_at(Opponent::Average);
        assert_eq!(
            &entries(&average.weight_table_scout)[..4],
            &[
                (UnitType::Artillery, 1),
                (UnitType::MissileLauncher, 1),
                (UnitType::GunTurret, 1),
                (UnitType::Submarine, 1),
            ]
        );
        assert_eq!(
            average
                .weight_table_ground_defense
                .get_weight(UnitType::Infiltrator),
            0
        );

        // EXPERT: COMMANDO/INFANTRY entries appear.
        let expert = player_at(Opponent::Expert);
        assert_eq!(
            expert
                .weight_table_ground_defense
                .get_weight(UnitType::Infiltrator),
            2
        );
        assert_eq!(
            expert.weight_table_scout.get_weight(UnitType::Infiltrator),
            1
        );
        assert_eq!(
            expert
                .weight_table_generic
                .get_weight(UnitType::Infiltrator),
            1
        );
        assert_eq!(
            expert.weight_table_generic.get_weight(UnitType::Infantry),
            1
        );
        assert_eq!(
            average.weight_table_generic.get_weight(UnitType::Infantry),
            0
        );
    }

    #[test]
    fn update_weight_tables_matches_the_cpp_strategy_switch() {
        // DEFENSIVE: GUNTURRT/ANTIMSSL/ANTIAIR ×3 in every table that holds them.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::Defensive;
        player.update_weight_tables();
        assert_eq!(player.weight_table_tanks.get_weight(UnitType::GunTurret), 3);
        assert_eq!(
            player
                .weight_table_air_defense
                .get_weight(UnitType::MissileLauncher),
            3
        );
        assert_eq!(
            player.weight_table_aircrafts.get_weight(UnitType::AntiAir),
            9
        );

        // MISSILES: MISSLLCH/MSSLBOAT ×5.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::Missiles;
        player.update_weight_tables();
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::MissileCrawler),
            10
        );
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::MissileCruiser),
            10
        );

        // AIR: FIGHTER/BOMBER ×5 (the `weight_table_bomber` table lists the
        // counters of bombers — FIGHTER — not BOMBER itself).
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::Air;
        player.update_weight_tables();
        assert_eq!(
            player.weight_table_aircrafts.get_weight(UnitType::Fighter),
            5
        );
        assert_eq!(player.weight_table_bomber.get_weight(UnitType::Fighter), 15);

        // SEA: FASTBOAT/CORVETTE/SUBMARNE/BATTLSHP/MSSLBOAT ×2.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::Sea;
        player.update_weight_tables();
        assert_eq!(
            player.weight_table_aircrafts.get_weight(UnitType::Escort),
            6
        );
        assert_eq!(
            player.weight_table_corvette.get_weight(UnitType::Corvette),
            2
        );
        assert_eq!(player.weight_table_scout.get_weight(UnitType::Submarine), 2);
        assert_eq!(player.weight_table_tanks.get_weight(UnitType::Gunboat), 4);
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::MissileCruiser),
            4
        );

        // SCOUT_HORDE: SCOUT is added to four tables, then ×10 everywhere.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::ScoutHorde;
        player.update_weight_tables();
        assert_eq!(player.weight_table_tanks.get_weight(UnitType::Scout), 10);
        assert_eq!(
            player.weight_table_assault_guns.get_weight(UnitType::Scout),
            20
        );
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::Scout),
            10
        );
        assert_eq!(
            player
                .weight_table_missile_launcher
                .get_weight(UnitType::Scout),
            10
        );
        assert_eq!(player.weight_table_scout.get_weight(UnitType::Scout), 20);

        // TANK_HORDE: TANK ×10.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::TankHorde;
        player.update_weight_tables();
        assert_eq!(player.weight_table_tanks.get_weight(UnitType::Tank), 20);
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::Tank),
            10
        );

        // FAST_ATTACK: ARTILLRY ×3, BOMBER/SP_FLAK/CORVETTE/FASTBOAT ×2.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::FastAttack;
        player.update_weight_tables();
        assert_eq!(
            player.weight_table_tanks.get_weight(UnitType::AssaultGun),
            3
        );
        assert_eq!(player.weight_table_tanks.get_weight(UnitType::Bomber), 6);
        assert_eq!(
            player
                .weight_table_aircrafts
                .get_weight(UnitType::MobileAntiAir),
            6
        );
        assert_eq!(
            player.weight_table_fastboat.get_weight(UnitType::Corvette),
            2
        );
        assert_eq!(
            player.weight_table_aircrafts.get_weight(UnitType::Escort),
            6
        );

        // COMBINED_ARMS: TANK/MISSLLCH ×5.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::CombinedArms;
        player.update_weight_tables();
        assert_eq!(player.weight_table_tanks.get_weight(UnitType::Tank), 10);
        assert_eq!(
            player
                .weight_table_tanks
                .get_weight(UnitType::MissileCrawler),
            10
        );

        // ESPIONAGE: SUBMARNE/COMMANDO ×4, MISSLLCH/MSSLBOAT ×2.
        // (The `weight_table_commando` table itself holds INFANTRY — the counter —
        // so COMMANDO is multiplied where it is listed, e.g. ground defense.)
        let mut player = player_at(Opponent::Expert);
        player.strategy = AiStrategy::Espionage;
        player.update_weight_tables();
        assert_eq!(player.weight_table_scout.get_weight(UnitType::Submarine), 4);
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::Infiltrator),
            8
        );
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::MissileCrawler),
            4
        );
        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::MissileCruiser),
            4
        );

        // RANDOM: no `default:` in C++, the tables stay untouched.
        let mut player = player_at(Opponent::Average);
        player.strategy = AiStrategy::Random;
        let before = entries(&player.weight_table_tanks);
        player.update_weight_tables();
        assert_eq!(entries(&player.weight_table_tanks), before);
    }

    #[test]
    fn rebuild_weight_tables_touches_all_eighteen_tables() {
        let mut player = player_at(Opponent::Expert);
        player.rebuild_weight_tables(UnitType::Tank, 3);

        for table in [
            &player.weight_table_ground_defense,
            &player.weight_table_scout,
            &player.weight_table_tanks,
            &player.weight_table_assault_guns,
            &player.weight_table_missile_launcher,
            &player.weight_table_air_defense,
            &player.weight_table_support_ships,
            &player.weight_table_infantry,
            &player.weight_table_generic,
        ] {
            assert!(table.get_weight(UnitType::Tank) > 0);
        }

        assert_eq!(
            player
                .weight_table_ground_defense
                .get_weight(UnitType::Tank),
            3
        );
        assert_eq!(player.weight_table_tanks.get_weight(UnitType::Tank), 6);

        // Tables without TANK keep their entries untouched.
        assert_eq!(
            entries(&player.weight_table_submarine),
            vec![(UnitType::Corvette, 1)]
        );
        assert_eq!(player.weight_table_fastboat.get_weight(UnitType::Tank), 0);
        assert_eq!(player.weight_table_corvette.get_weight(UnitType::Tank), 0);
        assert_eq!(
            player.weight_table_battleships.get_weight(UnitType::Tank),
            0
        );
    }

    #[test]
    fn ai_init_fills_the_tables_of_every_team() {
        let mut world = GameState::new();
        world.ai.settings.opponent = Opponent::Average as u8;

        crate::ai::ai_init(&mut world);

        for (index, player) in world.ai.players.iter().enumerate() {
            assert!(!player.weight_table_generic.is_empty(), "team {index}");
            assert!(!player.weight_table_tanks.is_empty(), "team {index}");
            assert_eq!(player.weight_table_tanks.team(), index as u8);
        }
    }

    #[test]
    fn begin_turn_applies_strategy_adjustments_once() {
        let mut world = GameState::new();
        world.ai.settings.opponent = Opponent::Average as u8;
        let team = TeamId::RED;
        world.teams[team.0 as usize].team_type = TeamKind::Computer;
        world.teams[team.0 as usize].team_clan = TeamClan::TheChosen;

        crate::ai::ai_init(&mut world);
        assert_eq!(
            world.ai.players[team.0 as usize]
                .weight_table_aircrafts
                .get_weight(UnitType::Fighter),
            1
        );

        // `BeginTurn`'s `need_init` block: clan THE_CHOSEN → AI_STRATEGY_AIR,
        // then `UpdateWeightTables` (FIGHTER/BOMBER ×5).
        crate::ai::ai_begin_turn(&mut world, team);
        let player = &world.ai.players[team.0 as usize];
        assert_eq!(player.strategy, AiStrategy::Air);
        assert_eq!(
            player.weight_table_aircrafts.get_weight(UnitType::Fighter),
            5
        );
        assert_eq!(player.weight_table_bomber.get_weight(UnitType::Fighter), 15);

        // The `need_init` block runs once: a second turn must not multiply again.
        crate::ai::ai_begin_turn(&mut world, team);
        assert_eq!(
            world.ai.players[team.0 as usize]
                .weight_table_aircrafts
                .get_weight(UnitType::Fighter),
            5
        );
    }
}
