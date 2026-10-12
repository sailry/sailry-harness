use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use std::sync::{Arc, Mutex};

fn query() -> Query {
    Query {
        kind: None,
        before: None,
        limit: 2,
    }
}

fn hit(sequence: u64) -> assets::Group {
    assets::Group {
        sequence,
        turn: TurnId::new(),
        worktree: WorktreeId::new(),
        timestamp_ms: 0,
        kind: assets::Kind::Artifact,
        items: vec![assets::Target::File {
            path: "report.md".into(),
        }],
    }
}

#[test]
fn unique_outputs_preserve_scope_and_latest_origin() {
    let newest = hit(30);
    let mut older = newest.clone();
    older.sequence = 20;
    older.turn = TurnId::new();
    older.items.push(assets::Target::File {
        path: "data.json".into(),
    });
    let mut input = newest.clone();
    input.sequence = 10;
    input.kind = assets::Kind::Resource;
    let mut elsewhere = newest.clone();
    elsewhere.sequence = 5;
    elsewhere.worktree = WorktreeId::new();
    let groups = [newest, older, input, elsewhere];
    let visible = unique(&groups)
        .map(|(group, index, _)| (group.sequence, index))
        .collect::<Vec<_>>();
    assert_eq!(visible, [(30, 0), (20, 1), (10, 0), (5, 0)]);
    assert_eq!(unique(&groups[..1]).count(), 1);
    assert_eq!(
        unique(&groups[..2]).count(),
        2,
        "loading older pages must not duplicate outputs"
    );
    assert_eq!(
        groups[1].items.len(),
        2,
        "canonical page metadata stays intact"
    );
}

struct Scans {
    pages: Mutex<std::collections::VecDeque<assets::Page>>,
    queries: Mutex<Vec<Query>>,
}

#[test]
fn document_ids_keep_the_newest_receipt() {
    let mut newest = hit(30);
    let document = sailry_protocol::artifact::Summary {
        kind: sailry_protocol::artifact::Kind::Document,
        id: ArtifactId::new(),
        session: SessionId::new(),
        turn: newest.turn,
        package: sailry_protocol::plugin::Reference {
            name: "document-fixture".into(),
            digest: "a".repeat(64),
            settings_revision: 0,
        },
        title: "Report".into(),
        summary: String::new(),
        revision: 2,
        size: 16,
        created_at: 0,
        updated_at: 1,
    };
    newest.items = vec![assets::Target::Document(document.clone())];
    let mut older = newest.clone();
    older.sequence = 20;
    older.worktree = WorktreeId::new();
    let mut previous = document.clone();
    previous.revision = 1;
    older.items = vec![assets::Target::Document(previous)];
    let mut same_title = document.clone();
    same_title.id = ArtifactId::new();
    older.items.push(assets::Target::Document(same_title));
    let groups = [newest, older];
    let visible = unique(&groups).collect::<Vec<_>>();
    assert_eq!(visible.len(), 2);
    assert_eq!(visible[0].2, &assets::Target::Document(document));
    assert_eq!((visible[1].0.sequence, visible[1].1), (20, 1));
    assert_eq!(unique(&groups[..1]).count(), 1);
    assert_eq!(groups[1].items.len(), 2);
}

impl Transport for Scans {
    fn target(&self) -> NodeId {
        NodeId([2; 32])
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let Command::ListConversationAssets { query, .. } = &request.command else {
                panic!("asset query expected")
            };
            assert!(!request.command.durable());
            self.queries.lock().unwrap().push(query.clone());
            let page = self.pages.lock().unwrap().pop_front().unwrap();
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(Ok(Output::ConversationAssets(page))).unwrap();
            Ok(Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            })
        })
    }
    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        unreachable!("asset inventory does not subscribe")
    }
}

#[tokio::test]
async fn fills_pages_across_empty_scans() {
    let session = SessionId::new();
    let scans = Arc::new(Scans {
        pages: Mutex::new(
            [
                assets::Page {
                    session,
                    groups: vec![],
                    revision: 1,
                    next_before: Some(100),
                },
                assets::Page {
                    session,
                    groups: vec![hit(90)],
                    revision: 1,
                    next_before: Some(50),
                },
                assets::Page {
                    session,
                    groups: vec![hit(40)],
                    revision: 1,
                    next_before: Some(40),
                },
            ]
            .into(),
        ),
        queries: Default::default(),
    });
    let result = Client::new(scans.clone())
        .conversation_assets(session, query())
        .await
        .unwrap();
    assert_eq!(
        result
            .groups
            .iter()
            .map(|found| found.sequence)
            .collect::<Vec<_>>(),
        [90, 40]
    );
    assert_eq!(result.next_before, Some(40));
    let queries = scans.queries.lock().unwrap();
    assert_eq!(
        queries.iter().map(|query| query.before).collect::<Vec<_>>(),
        [None, Some(100), Some(50)]
    );
    assert_eq!(
        queries.iter().map(|query| query.limit).collect::<Vec<_>>(),
        [2, 2, 1]
    );
}

#[test]
fn rejects_invalid_response_scope_order_and_filter() {
    let session = SessionId::new();
    let good = assets::Page {
        session,
        revision: 1,
        groups: vec![hit(9)],
        next_before: None,
    };
    assert!(validate(&good, session, &query()).is_ok());
    assert!(validate(&good, SessionId::new(), &query()).is_err());
    assert!(
        validate(
            &good,
            session,
            &Query {
                kind: Some(assets::Kind::Resource),
                ..query()
            }
        )
        .is_err()
    );
    let mut invalid_page = good.clone();
    invalid_page.groups.push(hit(9));
    assert!(validate(&invalid_page, session, &query()).is_err());
    invalid_page = good;
    invalid_page.next_before = Some(10);
    assert!(validate(&invalid_page, session, &query()).is_err());
}
#[tokio::test]
async fn rejects_revision_changes_between_scans() {
    let session = SessionId::new();
    let scans = Arc::new(Scans {
        pages: Mutex::new(
            [
                assets::Page {
                    session,
                    revision: 1,
                    groups: vec![hit(90)],
                    next_before: Some(50),
                },
                assets::Page {
                    session,
                    revision: 2,
                    groups: vec![hit(40)],
                    next_before: None,
                },
            ]
            .into(),
        ),
        queries: Default::default(),
    });
    assert_eq!(
        Client::new(scans)
            .conversation_assets(session, query())
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}
