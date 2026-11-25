use super::*;
use max_core::enums::{PlayMode, TeamClan, UnitType};
use max_core::geo::Point;
use max_core::input::Command;
use max_core::team::{TeamKind, TeamUnits};
use max_core::universe::{
    BarrierFacts, Dispatcher, GameUniverse, NetworkCommand, RemoteCommand, RemoteHandling,
    RoutedCommand,
};

/// Minimal dispatcher for the isolated audit tests (the audit itself is
/// dispatcher-agnostic: it only reads `snapshot`).
#[derive(Debug, Default)]
struct TestDispatcher;

impl Dispatcher for TestDispatcher {
    fn route(&mut self, cmd: Command) -> RoutedCommand {
        RoutedCommand::Local(cmd)
    }

    fn receive(&mut self, _cmd: RemoteCommand) -> RemoteHandling {
        RemoteHandling::Ignore
    }

    fn plan(&mut self, _facts: &BarrierFacts) -> Vec<NetworkCommand> {
        Vec::new()
    }
}

/// A world with one tank of the local team and a remote team slot.
fn world() -> (GameUniverse<TestDispatcher>, UnitId) {
    let mut universe = GameUniverse::with_dispatcher(TestDispatcher);
    *universe.team_units_mut(TeamId::RED).expect("economy") =
        TeamUnits::with_clan(TeamClan::Random);
    universe.team_mut(TeamId::RED).expect("slot").team_type = TeamKind::Player;
    let unit = universe
        .spawn_unit(UnitType::Tank, TeamId::RED, Point::new(10, 10), 0)
        .expect("tank");
    universe.set_play_mode(PlayMode::TurnBased);
    (universe, unit)
}

#[test]
fn audit_reply_matches_the_local_unit() {
    let (universe, unit) = world();
    let snapshot = universe.snapshot();
    let state = UnitState::from_unit(snapshot, snapshot.unit(unit).expect("unit"));
    let reply = audit_reply(snapshot, TeamId::RED, &Packet::Audit { unit, state });
    assert_eq!(
        reply,
        Some(Packet::AuditAck {
            team: TeamId::RED,
            status: true
        })
    );
}

#[test]
fn audit_reply_reports_a_field_difference_and_an_unknown_unit() {
    let (universe, unit) = world();
    let snapshot = universe.snapshot();
    let mut state = UnitState::from_unit(snapshot, snapshot.unit(unit).expect("unit"));
    state.hits = state.hits.wrapping_add(1);
    let reply = audit_reply(snapshot, TeamId::RED, &Packet::Audit { unit, state });
    assert_eq!(
        reply,
        Some(Packet::AuditAck {
            team: TeamId::RED,
            status: false
        })
    );

    let missing = UnitId(0x7FFF);
    let reply = audit_reply(
        snapshot,
        TeamId::RED,
        &Packet::Audit {
            unit: missing,
            state: UnitState::default(),
        },
    );
    assert_eq!(
        reply,
        Some(Packet::AuditAck {
            team: TeamId::RED,
            status: false
        })
    );
}

#[test]
fn audit_done_closes_the_wait_without_a_reply() {
    let (universe, _) = world();
    let snapshot = universe.snapshot();
    assert_eq!(
        audit_reply(
            snapshot,
            TeamId::RED,
            &Packet::AuditDone {
                team: TeamId::RED,
                param: 1
            }
        ),
        None
    );
}

#[test]
fn diff_lists_every_differing_field() {
    let (universe, unit) = world();
    let snapshot = universe.snapshot();
    let host = UnitState::from_unit(snapshot, snapshot.unit(unit).expect("unit"));
    let mut local = host;
    local.grid_x = local.grid_x.wrapping_add(1);
    local.ammo = local.ammo.wrapping_add(1);
    let diff = diff_unit_state(&host, &local);
    let names: Vec<&str> = diff.iter().map(|(name, ..)| *name).collect();
    assert_eq!(names, vec!["grid_x", "ammo"]);
}

#[test]
fn report_is_written_as_desync_turn_txt() {
    let mut audit = DesyncAudit::new(3, vec![TeamId(1), TeamId(2)]);
    audit.units = 14;
    audit.mismatches.push(AuditMismatch {
        unit: UnitId(42),
        team: TeamId(1),
    });
    let dir = std::env::temp_dir().join(format!(
            "max-brh-audit-test-{}-{}",
        std::process::id(),
        audit.turn
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    audit
        .write_report(&dir, 0x1234, &[(TeamId(1), 0x1250), (TeamId(2), 0x1234)])
        .expect("report");
    let path = dir.join("desync-turn3.txt");
    assert_eq!(audit.report.as_deref(), Some(path.as_path()));
    let text = std::fs::read_to_string(&path).expect("report text");
    assert!(text.contains("turn: 3"), "{text}");
    assert!(text.contains("own crc45: 0x1234"), "{text}");
    assert!(text.contains("peer crc45: [1:0x1250, 2:0x1234]"), "{text}");
    assert!(text.contains("mismatch: unit 42 team 1"), "{text}");
    let line = audit.to_line();
    assert!(line.contains("turn=3"), "{line}");
    assert!(line.contains("desync-turn3.txt"), "{line}");
    std::fs::remove_dir_all(&dir).ok();
}
