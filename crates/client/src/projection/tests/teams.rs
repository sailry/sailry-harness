use super::*;
use sailry_protocol::{RoleId, TeamId, team};

fn update(cursor: u64, event: Event) -> Update {
    Update::Event(EventEnvelope {
        node: NodeId([1; 32]),
        cursor,
        event,
    })
}

fn profile(key: &str, members: Vec<RoleId>) -> team::Profile {
    team::Profile {
        id: TeamId::new(),
        revision: 1,
        key: key.into(),
        name: key.into(),
        appearance: None,
        description: String::new(),
        model: None,
        instructions: String::new(),
        members,
    }
}

#[test]
fn recovers_ordered_identity_and_settings() {
    for (case_name, run) in [
        (
            "recovers_from_snapshot",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                let mut state = snapshot(2);
                let stored = profile("review", vec![RoleId::new()]);
                state.teams = vec![stored.clone()];
                projection.apply(1, Update::Snapshot(state)).unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().teams,
                    [stored],
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "updates_identity_and_order",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                projection.apply(1, Update::Snapshot(snapshot(0))).unwrap();
                let role = RoleId::new();
                let kept = RoleId::new();
                let mut review = profile("review", vec![role, kept]);
                let other = profile("analysis", vec![kept]);
                projection
                    .apply(1, update(1, Event::TeamChanged(review.clone())))
                    .unwrap();
                projection
                    .apply(1, update(2, Event::TeamChanged(other.clone())))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().teams,
                    [other.clone(), review.clone()],
                    "{case_name}"
                );
                review.members = vec![kept];
                review.revision = 2;
                projection
                    .apply(
                        1,
                        update(
                            3,
                            Event::RoleRemoved {
                                id: role,
                                teams: vec![review.clone()],
                            },
                        ),
                    )
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().teams,
                    [other.clone(), review.clone()],
                    "{case_name}"
                );
                projection
                    .apply(1, update(4, Event::TeamRemoved { id: review.id }))
                    .unwrap();
                assert_eq!(projection.snapshot().unwrap().teams, [other], "{case_name}");
            }) as fn(&str),
        ),
        (
            "tracks_settings_revision",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                let mut state = snapshot(0);
                state.team_settings_revision = 2;
                projection.apply(1, Update::Snapshot(state)).unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().team_settings_revision,
                    2,
                    "{case_name}"
                );
                projection
                    .apply(1, update(1, Event::TeamSettingsChanged { revision: 3 }))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().team_settings_revision,
                    3,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "tracks_compaction_settings_revision",
            (|case_name| {
                let mut projection = Projection::new(NodeId([1; 32]), 1);
                let mut state = snapshot(0);
                state.compaction_settings_revision = 2;
                projection.apply(1, Update::Snapshot(state)).unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().compaction_settings_revision,
                    2,
                    "{case_name}"
                );
                projection
                    .apply(
                        1,
                        update(1, Event::CompactionSettingsChanged { revision: 3 }),
                    )
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().compaction_settings_revision,
                    3,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
