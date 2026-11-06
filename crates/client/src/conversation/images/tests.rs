use super::*;
use sailry_protocol::{
    AttachmentId, SessionId, TurnId, WorktreeId,
    attachment::{Attachment, Spec},
    tool::detail::Summary,
};

fn image(entry: &str, index: usize) -> Image {
    Image {
        entry: entry.into(),
        part: 0,
        index,
        attachment: Attachment {
            id: AttachmentId::new(),
            spec: Spec {
                worktree: WorktreeId::new(),
                name: format!("image-{}.png", index + 1),
                media_type: "image/png".into(),
                size: 4,
                revision: "a".repeat(64),
            },
        },
    }
}

fn result(call: &str, images: Vec<Image>) -> Part {
    Part::ToolResult {
        id: Some(call.into()),
        name: "browser_screenshot".into(),
        summary: Box::new(Summary::default()),
        images,
    }
}

fn entry(id: &str, sequence: u64, author: &str, parts: Vec<Part>) -> Entry {
    Entry {
        sequence,
        id: id.into(),
        turn: TurnId::new(),
        author: author.into(),
        branch: String::new(),
        timestamp_ms: 0,
        parts,
        citations: vec![],
        search_suggestions: None,
        usage: None,
    }
}

fn page(entries: Vec<Entry>) -> Page {
    Page {
        session: SessionId::new(),
        revision: 1,
        entries: entries.into(),
        runs: vec![],
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        background: vec![],
        next_before: None,
    }
}

fn reference(call: &str, index: usize) -> Reference {
    Reference {
        call: call.into(),
        index,
    }
}

mod resolution {
    use super::*;

    #[test]
    fn matches_original_image_positions() {
        let second = image("capture", 2);
        let page = page(vec![entry(
            "capture",
            1,
            "assistant",
            vec![result("call_1", vec![image("capture", 0), second.clone()])],
        )]);
        assert_eq!(resolve(&page, &reference("call_1", 2)), Some(&second));
        assert_eq!(resolve(&page, &reference("call_1", 1)), None);
        assert_eq!(resolve(&page, &reference("call_2", 0)), None);
    }

    #[test]
    fn prefers_latest_result() {
        let latest = image("later", 0);
        let page = page(vec![
            entry(
                "earlier",
                1,
                "assistant",
                vec![result("call_1", vec![image("earlier", 0)])],
            ),
            entry(
                "later",
                2,
                "assistant",
                vec![result("call_1", vec![latest.clone()])],
            ),
        ]);
        assert_eq!(resolve(&page, &reference("call_1", 0)), Some(&latest));
    }

    #[test]
    fn ignores_other_parts() {
        let page = page(vec![entry(
            "message",
            1,
            "assistant",
            vec![
                Part::Image(image("message", 0)),
                Part::ToolResult {
                    id: None,
                    name: "browser_screenshot".into(),
                    summary: Box::new(Summary::default()),
                    images: vec![image("message", 0)],
                },
            ],
        )]);
        assert_eq!(resolve(&page, &reference("message", 0)), None);
    }
}

mod markdown {
    use super::*;

    #[test]
    fn finds_destinations_in_order() {
        let text = "Before ![Saved](sailry-image:call_1/0) and ![Other](<SAILRY-IMAGE:call%202/3>)\n\
                    [link](sailry-image:call_3/1 \"Title\") sailry-image:bad sailry-image:call_4/01";
        let found: Vec<_> = spans(text)
            .into_iter()
            .map(|(range, reference)| (text[range].to_owned(), reference))
            .collect();
        assert_eq!(
            found,
            vec![
                ("sailry-image:call_1/0".into(), reference("call_1", 0)),
                ("SAILRY-IMAGE:call%202/3".into(), reference("call 2", 3)),
                ("sailry-image:call_3/1".into(), reference("call_3", 1)),
            ]
        );
        assert!(spans("中文 🙂 without references").is_empty());
    }

    #[test]
    fn resolves_assistant_references_once() {
        let captured = image("capture", 0);
        let mut page = page(vec![
            entry(
                "question",
                1,
                "user",
                vec![Part::Text("![User](sailry-image:call_1/0)".into())],
            ),
            entry(
                "capture",
                2,
                "assistant",
                vec![result("call_1", vec![captured.clone()])],
            ),
            entry(
                "answer",
                3,
                "assistant",
                vec![Part::Text(
                    "![Changed](sailry-image:call_1/0) ![Again](sailry-image:call_1/0) ![Missing](sailry-image:call_9/0)"
                        .into(),
                )],
            ),
        ]);
        let expected = vec![Resolved {
            call: "call_1".into(),
            index: 0,
            image: captured.clone(),
        }];
        assert_eq!(referenced(&page, &[]), expected);
        std::sync::Arc::make_mut(&mut page.entries).truncate(2);
        assert!(referenced(&page, &[]).is_empty());
        let drafts = [Draft {
            id: "draft".into(),
            turn: TurnId::new(),
            author: "assistant".into(),
            branch: String::new(),
            parts: vec![Part::Text("![Streaming](sailry-image:call_1/0)".into())],
        }];
        assert_eq!(referenced(&page, &drafts), expected);
        assert_eq!(
            serde_json::to_value(&expected[0]).unwrap()["call"],
            "call_1"
        );
    }
}
