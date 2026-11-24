//! Packet **09** — team state (`gold`, `gold_spent_on_upgrades`, research
//! allocation) through the command channel.
//!
//! Oracle: `Remote_ReceiveNetPacket_09` (`max-cpp-port/src/remote.cpp:2088-2107`)
//! assigns the absolute gold, the absolute cumulative upgrade spending and the
//! research topics; the allocation field is what `ResearchMenu_OnClick_Done`
//! (`researchmenu.cpp:518-556`) computes before it sends 09. In the port the menu
//! emits the command instead of writing the state, so `apply_packet_09` runs that
//! same `Done` loop (`GameState::set_research_allocation`).
//!
//! These tests pin the apply semantics: field assignment, center reassignment,
//! power-off of released centers, the "gold only" commit that must not touch the
//! centers, and the invalid team index.

use max_core::enums::{ResearchTopic, TeamClan, UnitOrder, UnitOrderState, UnitType};
use max_core::events::ApplyOutcome;
use max_core::geo::Point;
use max_core::input::{Command, Packet09TeamState};
use max_core::team::{ResearchTopicInfo, TeamId, TeamKind, TeamUnits};
use max_core::universe::{GameUniverse, LocalDispatcher};

/// A universe with the RED team's economy and one powered-on research center per
/// topic entry of `topics`.
fn universe_with_centres(
    topics: &[ResearchTopic],
) -> (GameUniverse<LocalDispatcher>, Vec<max_core::UnitId>) {
    let mut universe = GameUniverse::default();
    universe.teams_mut()[TeamId::RED.0 as usize].team_type = TeamKind::Player;
    let mut units = TeamUnits::with_clan(TeamClan::Random);
    units.gold = 500;
    *universe.team_units_mut(TeamId::RED).expect("RED economy") = units;

    let mut ids = Vec::new();
    for (index, topic) in topics.iter().enumerate() {
        let id = universe
            .spawn_unit(
                UnitType::ResearchCentre,
                TeamId::RED,
                Point::new(10 + index as i16 * 3, 10),
                0,
            )
            .expect("research center");
        let mut unit = universe.snapshot().unit(id).cloned().expect("unit");
        unit.orders = UnitOrder::PowerOn;
        unit.state = UnitOrderState::ExecutingOrder;
        unit.research_topic = *topic;
        universe.write_unit(id, unit);
        universe.update_research_progress(TeamId::RED, *topic, 1);
        ids.push(id);
    }

    (universe, ids)
}

/// The packet the research menu emits: the committed allocation, the current
/// level/turns and the unchanged economy.
fn commit(universe: &GameUniverse<LocalDispatcher>, targets: [u32; 8]) -> Command {
    let info = &universe.snapshot().teams()[TeamId::RED.0 as usize];
    let mut research = info.research_topics;
    for (topic, count) in research.iter_mut().zip(targets.iter()) {
        topic.allocation = *count;
    }

    Command::TeamState(Packet09TeamState {
        team: TeamId::RED,
        gold: universe.snapshot().team_units()[TeamId::RED.0 as usize].gold,
        gold_spent_on_upgrades: info.stats_gold_spent_on_upgrades,
        research,
        name: "RED".to_string(),
    })
}

fn allocation(universe: &GameUniverse<LocalDispatcher>) -> [u32; 8] {
    universe.snapshot().research_allocation(TeamId::RED)
}

#[test]
fn packet_09_assigns_gold_and_cumulative_upgrade_spending() {
    let (mut universe, _) = universe_with_centres(&[ResearchTopic::Attack]);
    let mut command = commit(&universe, [1, 0, 0, 0, 0, 0, 0, 0]);

    let Command::TeamState(packet) = &mut command else {
        unreachable!()
    };
    packet.gold = 123;
    packet.gold_spent_on_upgrades = 45;

    assert_eq!(universe.apply_now(command), ApplyOutcome::Applied);

    let snapshot = universe.snapshot();
    assert_eq!(snapshot.team_units()[TeamId::RED.0 as usize].gold, 123);
    assert_eq!(
        snapshot.teams()[TeamId::RED.0 as usize].stats_gold_spent_on_upgrades,
        45
    );
    // The allocation did not change, so the center stays where it was.
    let unit = snapshot.unit(snapshot.unit_ids_in_order()[0]).unwrap();
    assert_eq!(unit.research_topic, ResearchTopic::Attack);
    assert_eq!(unit.orders, UnitOrder::PowerOn);
}

#[test]
fn packet_09_reassigns_the_centres_to_the_committed_allocation() {
    // Two centers on Attack; the commit keeps one slot on Attack and gives one to
    // Shots.
    let (mut universe, ids) =
        universe_with_centres(&[ResearchTopic::Attack, ResearchTopic::Attack]);
    assert_eq!(allocation(&universe)[ResearchTopic::Attack as usize], 2);

    let command = commit(&universe, [1, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(universe.apply_now(command), ApplyOutcome::Applied);

    let snapshot = universe.snapshot();
    // C++ `ResearchMenu_OnClick_Done`: a center keeps its topic while the count
    // lasts, the surplus one moves to the first topic with room.
    let mut topics: Vec<ResearchTopic> = ids
        .iter()
        .map(|id| snapshot.unit(*id).expect("center").research_topic)
        .collect();
    topics.sort_by_key(|topic| *topic as u8);
    assert_eq!(
        topics,
        vec![ResearchTopic::Attack, ResearchTopic::Shots],
        "one center keeps Attack (count 1), the other moves to Shots"
    );
    for id in &ids {
        assert_eq!(snapshot.unit(*id).unwrap().orders, UnitOrder::PowerOn);
    }

    let allocation = allocation(&universe);
    assert_eq!(allocation[ResearchTopic::Attack as usize], 1);
    assert_eq!(allocation[ResearchTopic::Shots as usize], 1);
}

#[test]
fn packet_09_powers_off_centres_without_a_slot() {
    let (mut universe, ids) = universe_with_centres(&[ResearchTopic::Attack, ResearchTopic::Shots]);

    // Release everything: no topic has room, both centers are turned off.
    let command = commit(&universe, [0; 8]);
    assert_eq!(universe.apply_now(command), ApplyOutcome::Applied);

    let snapshot = universe.snapshot();
    for id in &ids {
        assert_eq!(snapshot.unit(*id).unwrap().orders, UnitOrder::PowerOff);
    }
    // The allocation is written off when the power-off order is processed
    // (`UnitInfo::PowerDown` → `UpdateResearchProgress(-1)`), exactly as in C++.
    assert_eq!(allocation(&universe).iter().sum::<u32>(), 2);

    universe.tick();
    assert_eq!(allocation(&universe).iter().sum::<u32>(), 0);
}

#[test]
fn packet_09_with_the_same_allocation_is_idempotent() {
    let (mut universe, ids) = universe_with_centres(&[ResearchTopic::Attack, ResearchTopic::Shots]);
    let before = universe.snapshot().snapshot_bytes();

    let command = commit(&universe, allocation(&universe));
    assert_eq!(universe.apply_now(command), ApplyOutcome::Applied);
    assert_eq!(
        universe.snapshot().snapshot_bytes(),
        before,
        "a commit that changes nothing must not touch the state"
    );

    for id in &ids {
        assert_eq!(
            universe.snapshot().unit(*id).unwrap().orders,
            UnitOrder::PowerOn
        );
    }
}

#[test]
fn invalid_team_index_is_ignored() {
    let (mut universe, _) = universe_with_centres(&[ResearchTopic::Attack]);
    let before = universe.snapshot().snapshot_bytes();

    let command = Command::TeamState(Packet09TeamState {
        team: TeamId(9),
        gold: 0,
        gold_spent_on_upgrades: 0,
        research: [ResearchTopicInfo::default(); 8],
        name: "nobody".to_string(),
    });

    assert_eq!(universe.apply_now(command), ApplyOutcome::Ignored);
    assert_eq!(universe.snapshot().snapshot_bytes(), before);
}
