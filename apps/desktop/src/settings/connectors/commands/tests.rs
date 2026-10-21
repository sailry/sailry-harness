use super::{Submission, finish, observed_edit, prepare, reconcile, success_key};
use sailry_protocol::{
    Command, ErrorCode, NodeId, Request, RequestId,
    plugin::connector::{self, Connection},
};
use std::collections::{BTreeMap, BTreeSet};

fn row(id: connector::Id, revision: u64) -> Connection {
    Connection {
        id,
        package: sailry_protocol::plugin::Reference {
            name: "fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        connector: "account".into(),
        method: connector::Kind::Secret,
        revision,
        enabled: true,
        state: connector::State::Connected,
        can_disconnect: true,
        identity: None,
        expires_at_ms: None,
        checked_at_ms: None,
        fields: BTreeMap::new(),
        configured: vec!["token".into()],
        consent: None,
    }
}

#[test]
fn releases_only_observed_older_guards() {
    let id = RequestId::new();
    let other = RequestId::new();
    let request = Request::new(
        NodeId([9; 32]),
        Command::SetConnectionEnabled {
            connection: id,
            expected_revision: 7,
            enabled: false,
        },
    );
    let request_id = request.id;
    let mut requests = BTreeMap::from([(
        id,
        Submission {
            request,
            revision: 7,
        },
    )]);
    let mut pending = BTreeSet::new();
    for rows in [
        vec![],
        vec![row(id, 7)],
        vec![row(id, 6)],
        vec![row(other, 8)],
    ] {
        reconcile(&mut requests, &pending, &rows);
        assert_eq!(requests[&id].request.id, request_id);
    }
    pending.insert(id);
    let rows = [row(id, 8)];
    reconcile(&mut requests, &pending, &rows);
    assert_eq!(requests[&id].request.id, request_id);
    // The snapshot may arrive before the transport reports an uncertain result.
    pending.remove(&id);
    reconcile(&mut requests, &pending, &rows);
    assert!(requests.is_empty());
}

#[test]
fn tracks_probe_revisions_without_changing_commands() {
    let id = RequestId::new();
    let other = RequestId::new();
    let request = Request::new(NodeId([9; 32]), Command::ProbeConnection { connection: id });
    let mut requests = BTreeMap::from([
        (
            id,
            Submission {
                request,
                revision: 3,
            },
        ),
        (
            other,
            Submission {
                request: Request::new(
                    NodeId([9; 32]),
                    Command::DisconnectConnection {
                        connection: other,
                        expected_revision: 9,
                    },
                ),
                revision: 9,
            },
        ),
    ]);
    reconcile(
        &mut requests,
        &BTreeSet::new(),
        &[row(id, 4), row(other, 9)],
    );
    assert!(!requests.contains_key(&id));
    assert_eq!(requests.len(), 1);
    assert!(matches!(
        requests[&other].request.command,
        Command::DisconnectConnection {
            expected_revision: 9,
            ..
        }
    ));
}

#[test]
fn releases_edits_without_replacing_login_attempts() {
    let id = RequestId::new();
    let request = Request::new(
        NodeId([9; 32]),
        Command::UpdateConnection {
            connection: id,
            expected_revision: 7,
            input: connector::Input::default(),
        },
    );
    assert!(observed_edit(&request, &[]).is_none());
    assert!(observed_edit(&request, &[row(id, 7)]).is_none());
    assert!(observed_edit(&request, &[row(RequestId::new(), 8)]).is_none());
    assert_eq!(observed_edit(&request, &[row(id, 8)]).unwrap().revision, 8);
    let row = row(id, 8);
    let login = Request::new(
        NodeId([9; 32]),
        Command::StartConnectorLogin {
            package: row.package.clone(),
            connector: row.connector.clone(),
            connection: Some(id),
            expected_revision: Some(7),
            method: row.method,
            input: connector::Input::default(),
        },
    );
    assert!(observed_edit(&login, &[row]).is_none());
}

#[test]
fn prepares_fresh_probe_requests() {
    let id = RequestId::new();
    let mut requests = BTreeMap::new();
    let first = prepare(
        &mut requests,
        id,
        3,
        Request::new(NodeId([9; 32]), Command::ProbeConnection { connection: id }),
    );
    let second = prepare(
        &mut requests,
        id,
        3,
        Request::new(NodeId([9; 32]), Command::ProbeConnection { connection: id }),
    );
    assert_ne!(first.id, second.id);
    assert!(requests.is_empty());
    let durable = prepare(
        &mut requests,
        id,
        3,
        Request::new(
            NodeId([9; 32]),
            Command::DisconnectConnection {
                connection: id,
                expected_revision: 3,
            },
        ),
    );
    finish(
        &mut requests,
        id,
        durable.id,
        Some(ErrorCode::OutcomeUnknown),
    );
    finish(&mut requests, id, first.id, Some(ErrorCode::Unavailable));
    assert_eq!(requests[&id].request.id, durable.id);
    finish(&mut requests, id, durable.id, Some(ErrorCode::Unavailable));
    assert!(requests.is_empty());
}

#[test]
fn retains_only_unknown_durable_outcomes() {
    for probe in [false, true] {
        for code in [
            None,
            Some(ErrorCode::Unavailable),
            Some(ErrorCode::OutcomeUnknown),
        ] {
            let id = RequestId::new();
            let command = if probe {
                Command::ProbeConnection { connection: id }
            } else {
                Command::DisconnectConnection {
                    connection: id,
                    expected_revision: 3,
                }
            };
            let request = Request::new(NodeId([9; 32]), command);
            let request_id = request.id;
            let mut requests = BTreeMap::new();
            let prepared = prepare(&mut requests, id, 3, request);
            assert_eq!(prepared.id, request_id);
            assert_eq!(requests.contains_key(&id), !probe);
            finish(&mut requests, id, request_id, code);
            assert_eq!(
                requests.contains_key(&id),
                !probe && code == Some(ErrorCode::OutcomeUnknown),
                "{probe}/{code:?}",
            );
        }
    }
}

#[test]
fn reports_the_completed_operation() {
    let connection = RequestId::new();
    for (command, key) in [
        (Command::ProbeConnection { connection }, "connector_checked"),
        (
            Command::DisconnectConnection {
                connection,
                expected_revision: 7,
            },
            "connector_disconnected",
        ),
        (
            Command::RemoveConnection {
                connection,
                expected_revision: 7,
            },
            "connector_removed",
        ),
        (
            Command::SetConnectionEnabled {
                connection,
                expected_revision: 7,
                enabled: false,
            },
            "connector_saved",
        ),
        (
            Command::ConfirmConnectorConsent {
                connection,
                expected_revision: 7,
                accept: true,
            },
            "connector_saved",
        ),
    ] {
        assert_eq!(success_key(&command), key);
    }
}
