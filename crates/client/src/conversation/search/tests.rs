use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use std::sync::{Arc, Mutex};

fn query() -> Query {
    Query {
        text: "needle".into(),
        case_sensitive: false,
        before: None,
        limit: 2,
    }
}

fn hit(sequence: u64) -> search::Match {
    search::Match {
        turn: TurnId::new(),
        turn_sequence: sequence,
        entry: format!("entry-{sequence}"),
        sequence,
        part: 0,
        author: "assistant".into(),
        snippet: "needle 中文".into(),
        highlight: 0..6,
    }
}

struct Scans {
    pages: Mutex<std::collections::VecDeque<search::Page>>,
    queries: Mutex<Vec<Query>>,
}

impl Transport for Scans {
    fn target(&self) -> NodeId {
        NodeId([2; 32])
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let Command::SearchConversation { query, .. } = &request.command else {
                panic!("search expected")
            };
            assert!(!request.command.durable());
            self.queries.lock().unwrap().push(query.clone());
            let page = self.pages.lock().unwrap().pop_front().unwrap();
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(Ok(Output::ConversationMatches(page))).unwrap();
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
        unreachable!("search does not subscribe")
    }
}

#[tokio::test]
async fn fills_pages_across_empty_scans() {
    let session = SessionId::new();
    let scans = Arc::new(Scans {
        pages: Mutex::new(
            [
                search::Page {
                    session,
                    matches: vec![],
                    revision: 1,
                    next_before: Some(100),
                },
                search::Page {
                    session,
                    matches: vec![hit(90)],
                    revision: 1,
                    next_before: Some(50),
                },
                search::Page {
                    session,
                    matches: vec![hit(40)],
                    revision: 1,
                    next_before: Some(40),
                },
            ]
            .into(),
        ),
        queries: Default::default(),
    });
    let result = Client::new(scans.clone())
        .search_conversation(session, query())
        .await
        .unwrap();
    assert_eq!(
        result
            .matches
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

#[tokio::test]
async fn rejects_changed_history() {
    let session = SessionId::new();
    let scans = Arc::new(Scans {
        pages: Mutex::new(
            [
                search::Page {
                    session,
                    revision: 1,
                    matches: vec![hit(20)],
                    next_before: Some(20),
                },
                search::Page {
                    session,
                    revision: 2,
                    matches: vec![hit(10)],
                    next_before: None,
                },
            ]
            .into(),
        ),
        queries: Default::default(),
    });
    assert_eq!(
        Client::new(scans)
            .search_conversation(session, query())
            .await
            .unwrap_err()
            .code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn rejects_invalid_pages() {
    let session = SessionId::new();
    let valid = search::Page {
        session,
        revision: 1,
        matches: vec![hit(9), hit(8)],
        next_before: Some(7),
    };
    assert!(validate(&valid, session, &query()).is_ok());
    assert!(validate(&valid, SessionId::new(), &query()).is_err());
    let mut wrong = valid.clone();
    wrong.matches.swap(0, 1);
    assert!(validate(&wrong, session, &query()).is_err());
    let mut wrong = valid.clone();
    wrong.matches[0].highlight = 6..8;
    assert!(validate(&wrong, session, &query()).is_err());
    let mut wrong = valid.clone();
    wrong.matches[1].entry = wrong.matches[0].entry.clone();
    assert!(validate(&wrong, session, &query()).is_err());
    for next in [0, 9, i64::MAX as u64] {
        let mut wrong = valid.clone();
        wrong.next_before = Some(next);
        assert!(validate(&wrong, session, &query()).is_err());
    }
    let empty = search::Page {
        session,
        revision: 1,
        matches: vec![],
        next_before: Some(10),
    };
    assert!(
        validate(
            &empty,
            session,
            &Query {
                before: Some(10),
                ..query()
            }
        )
        .is_err()
    );
}

#[tokio::test]
async fn rejects_duplicate_results() {
    let session = SessionId::new();
    let first = hit(10);
    let mut repeated = hit(5);
    repeated.entry = first.entry.clone();
    let scans = Arc::new(Scans {
        pages: Mutex::new(
            [
                search::Page {
                    session,
                    matches: vec![first],
                    revision: 1,
                    next_before: Some(10),
                },
                search::Page {
                    session,
                    matches: vec![repeated],
                    revision: 1,
                    next_before: None,
                },
            ]
            .into(),
        ),
        queries: Default::default(),
    });
    assert!(
        Client::new(scans)
            .search_conversation(session, query())
            .await
            .is_err()
    );
}
