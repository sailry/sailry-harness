use super::*;
use sailry_protocol::{
    NodeId, SessionId, WorktreeId,
    conversation::{Draft, Entry, Page, Part, Pending, Run, RunKind, SentInput, Snapshot, Status},
};
use serde_json::json;
use std::sync::Arc;

fn owner() -> Run {
    Run {
        worktree: WorktreeId::new(),
        turn: TurnId::new(),
        kind: RunKind::Task,
        session: SessionId::new(),
        sequence: 1,
        revision: 1,
        status: Status::Running,
        error: None,
        started_ms: Some(1),
        finished_ms: None,
        origin: None,
        continuation: None,
        guidance: None,
        joined: None,
        notice: false,
        notices: Vec::new(),
        input: Some(SentInput {
            message: "Initial task".into(),
            attachments: Vec::new(),
        }),
    }
}

/// Guidance claimed by `owner`; delivery also needs its canonical user entry.
fn guide(owner: &Run, sequence: u64, text: &str) -> Run {
    Run {
        turn: TurnId::new(),
        sequence,
        status: Status::Continued,
        started_ms: None,
        finished_ms: Some(sequence as i64),
        guidance: Some(owner.turn),
        joined: Some(owner.turn),
        input: Some(SentInput {
            message: text.into(),
            attachments: Vec::new(),
        }),
        ..owner.clone()
    }
}

/// Queued input joined into a successor without guidance.
fn joined(owner: &Run, sequence: u64, text: &str) -> Run {
    Run {
        guidance: None,
        ..guide(owner, sequence, text)
    }
}

fn entry(sequence: u64, turn: TurnId, author: &str, text: &str) -> Entry {
    Entry {
        sequence,
        id: format!("entry-{sequence}"),
        turn,
        author: author.into(),
        branch: String::new(),
        timestamp_ms: sequence as i64,
        parts: vec![Part::Text(text.into())],
        citations: Vec::new(),
        search_suggestions: None,
        usage: None,
    }
}

fn page(runs: Vec<Run>, entries: Vec<Entry>) -> Page {
    Page {
        session: runs[0].session,
        revision: 1,
        entries: Arc::new(entries),
        runs,
        queue: Default::default(),
        approvals: Vec::new(),
        questions: Vec::new(),
        children: Vec::new(),
        background: Vec::new(),
        next_before: None,
    }
}

fn view(page: Page, drafts: Vec<Draft>) -> View {
    View {
        guidance: Arc::new(guidance::project(&page)),
        timeline: Arc::new(turns::timeline(&page)),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 1,
            page: Arc::new(page),
            missing: Vec::new(),
            drafts,
            statistics: None,
            artifacts: Default::default(),
        })),
        ..Default::default()
    }
}

/// Input and row texts of each segment.
fn outline<'a>(segments: &'a [Segment<'_>]) -> Vec<(Vec<&'a str>, Vec<&'a str>)> {
    segments
        .iter()
        .map(|segment| {
            (
                segment
                    .inputs
                    .iter()
                    .map(|input| input.content.input.message.text.as_str())
                    .collect(),
                segment
                    .rows
                    .iter()
                    .map(|row| row.text.as_deref().unwrap_or_default())
                    .collect(),
            )
        })
        .collect()
}

mod segmentation {
    use super::*;

    #[test]
    fn splits_reply_at_delivered_guidance() {
        let owner = owner();
        let guide = guide(&owner, 2, "Use the fixture");
        let view = view(
            page(
                vec![owner.clone(), guide.clone()],
                vec![
                    entry(1, owner.turn, "user", "Initial task"),
                    entry(2, owner.turn, "assistant", "Reply A"),
                    entry(3, guide.turn, "user", "Use the fixture"),
                    entry(4, owner.turn, "assistant", "Reply B"),
                ],
            ),
            Vec::new(),
        );
        let split = segments(&view, owner.turn);
        assert_eq!(
            outline(&split),
            [
                (vec!["Initial task"], vec!["Reply A"]),
                (vec!["Use the fixture"], vec!["Reply B"]),
            ]
        );
        assert_eq!(split[0].inputs[0].turn, owner.turn);
        assert_eq!(split[1].inputs[0].turn, guide.turn);
        let encoded = serde_json::to_value(&split).unwrap();
        assert_eq!(encoded[1]["inputs"][0]["turn"], json!(guide.turn));
        assert_eq!(encoded[1]["inputs"][0]["canonical"], true);
        assert_eq!(
            encoded[1]["inputs"][0]["message"]["text"],
            "Use the fixture"
        );
        assert_eq!(encoded[1]["rows"][0]["part"]["data"], "Reply B");
        assert!(
            segments(&view, guide.turn)
                .iter()
                .all(|segment| segment.rows.is_empty())
        );
    }

    #[test]
    fn shares_one_boundary() {
        let owner = owner();
        let first = guide(&owner, 2, "First guide");
        let second = guide(&owner, 3, "Second guide");
        let view = view(
            page(
                vec![owner.clone(), first.clone(), second.clone()],
                vec![
                    entry(1, owner.turn, "user", "Initial task"),
                    entry(2, owner.turn, "assistant", "Reply A"),
                    entry(3, first.turn, "user", "First guide"),
                    entry(4, second.turn, "user", "Second guide"),
                    entry(5, owner.turn, "assistant", "Reply B"),
                ],
            ),
            Vec::new(),
        );
        assert_eq!(
            outline(&segments(&view, owner.turn)),
            [
                (vec!["Initial task"], vec!["Reply A"]),
                (vec!["First guide", "Second guide"], vec!["Reply B"]),
            ]
        );
    }

    #[test]
    fn opens_successor_with_every_initial_input() {
        let owner = owner();
        let first = joined(&owner, 2, "First queued");
        let second = joined(&owner, 3, "Second queued");
        let view = view(
            page(
                vec![first.clone(), second.clone(), owner.clone()],
                vec![
                    entry(1, owner.turn, "user", "Initial task"),
                    entry(2, first.turn, "user", "First queued"),
                    entry(3, second.turn, "user", "Second queued"),
                    entry(4, owner.turn, "assistant", "Reply"),
                ],
            ),
            Vec::new(),
        );
        assert_eq!(
            outline(&segments(&view, owner.turn)),
            [(
                vec!["Initial task", "First queued", "Second queued"],
                vec!["Reply"],
            )]
        );
    }

    #[test]
    fn places_drafts_last() {
        let owner = owner();
        let guide = guide(&owner, 2, "Use the fixture");
        let draft = Draft {
            id: "draft".into(),
            turn: owner.turn,
            author: "assistant".into(),
            branch: String::new(),
            parts: vec![Part::Text("Streaming B".into())],
        };
        let view = view(
            page(
                vec![owner.clone(), guide.clone()],
                vec![
                    entry(1, owner.turn, "user", "Initial task"),
                    entry(2, owner.turn, "assistant", "Reply A"),
                    entry(3, guide.turn, "user", "Use the fixture"),
                ],
            ),
            vec![draft],
        );
        let split = segments(&view, owner.turn);
        assert_eq!(
            outline(&split),
            [
                (vec!["Initial task"], vec!["Reply A"]),
                (vec!["Use the fixture"], vec!["Streaming B"]),
            ]
        );
        assert!(split[1].rows[0].draft);
    }

    #[test]
    fn keeps_claimed_input_last() {
        let owner = owner();
        let guide = guide(&owner, 2, "Use the fixture");
        let claimed = joined(&owner, 3, "Claimed input");
        let mut entries = vec![
            entry(1, owner.turn, "user", "Initial task"),
            entry(2, owner.turn, "assistant", "Reply A"),
        ];
        let before = view(
            page(vec![owner.clone(), claimed.clone()], entries.clone()),
            Vec::new(),
        );
        assert_eq!(
            outline(&segments(&before, owner.turn)),
            [(vec!["Initial task", "Claimed input"], vec!["Reply A"])]
        );
        entries.extend([
            entry(3, guide.turn, "user", "Use the fixture"),
            entry(4, owner.turn, "assistant", "Reply B"),
        ]);
        let after = view(
            page(vec![owner.clone(), guide, claimed], entries),
            Vec::new(),
        );
        let split = segments(&after, owner.turn);
        assert_eq!(
            outline(&split),
            [
                (vec!["Initial task"], vec!["Reply A"]),
                (vec!["Use the fixture", "Claimed input"], vec!["Reply B"]),
            ]
        );
        assert!(!split[1].inputs[1].content.canonical);
    }

    #[test]
    fn omits_pending_guidance() {
        let owner = owner();
        let claimed = guide(&owner, 2, "Claimed guidance");
        let queued = Run {
            status: Status::Queued,
            joined: None,
            finished_ms: None,
            ..guide(&owner, 3, "Queued guidance")
        };
        let mut page = page(
            vec![owner.clone(), claimed.clone(), queued.clone()],
            vec![
                entry(1, owner.turn, "user", "Initial task"),
                entry(2, owner.turn, "assistant", "Reply A"),
            ],
        );
        page.queue.items.push(Pending {
            turn: queued.turn,
            revision: 1,
            config_revision: 1,
            ready: true,
            guidance: Some(owner.turn),
            notice: false,
            request: None,
            kind: RunKind::Task,
            preview: "Queued guidance".into(),
            truncated: false,
            attachments: Vec::new(),
        });
        let view = view(page, Vec::new());
        assert_eq!(view.guidance[&claimed.turn], guidance::State::Pending);
        assert_eq!(view.guidance[&queued.turn], guidance::State::Pending);
        assert_eq!(
            outline(&segments(&view, owner.turn)),
            [(vec!["Initial task"], vec!["Reply A"])]
        );
        assert_eq!(owners(&view), [owner.turn]);
    }
}

mod timeline {
    use super::*;

    #[test]
    fn lists_visible_owners_in_order() {
        let owner = owner();
        let guide = guide(&owner, 2, "Use the fixture");
        let queued = Run {
            turn: TurnId::new(),
            sequence: 3,
            status: Status::Queued,
            started_ms: None,
            input: Some(SentInput {
                message: "Separate task".into(),
                attachments: Vec::new(),
            }),
            ..owner.clone()
        };
        let entries = vec![
            entry(1, owner.turn, "user", "Initial task"),
            entry(2, guide.turn, "user", "Use the fixture"),
        ];
        let loaded = view(
            page(
                vec![owner.clone(), guide.clone(), queued.clone()],
                entries.clone(),
            ),
            Vec::new(),
        );
        assert_eq!(owners(&loaded), [owner.turn, queued.turn]);
        let unloaded = view(
            page(vec![guide.clone(), queued.clone()], entries),
            Vec::new(),
        );
        assert_eq!(owners(&unloaded), [guide.turn, queued.turn]);
        assert!(owners(&View::default()).is_empty());
    }
}
