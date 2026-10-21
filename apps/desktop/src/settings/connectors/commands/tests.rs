use super::{Submission, observed_edit, reconcile};
use sailry_protocol::{
    Command, NodeId, Request, RequestId,
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
