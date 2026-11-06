use super::*;
use crate::conversation::live::recovery::Identity;
use core::prelude::v1::test;
use sailry_protocol::{
    NodeId, ProjectId, SessionId, WorktreeId,
    conversation::reference::{Reference, Target},
};

fn snapshot(node: NodeId, session: Option<SessionId>) -> Snapshot {
    Snapshot {
        draft: Draft {
            identity: Identity {
                node,
                session,
                project: Some(ProjectId::new()),
                worktree: Some(WorktreeId::new()),
                resource: None,
                assistant: None,
            },
            text: "Continue with @资料\nUnicode draft 🦀".into(),
            tokens: vec![],
            selection: 0..0,
            references: vec![Reference {
                label: "@资料".into(),
                target: Target::File("资料.md".into()),
            }],
            commands: Default::default(),
            attachments: vec![],
            chips: vec![],
            configuration: None,
            configuration_owner: node,
            mode: Some(sailry_protocol::WorkMode::Plan),
            permission: Some(sailry_protocol::Permission::Project),
        },
        sources: vec![],
    }
}

#[test]
fn scoped_cache_and_drafts() {
    for (scenario, check) in [
        (
            "preserves_scoped_references_and_attachment_bytes",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let inputs = tempfile::tempdir().unwrap();
                let file = inputs.path().join("资料.bin");
                fs::write(&file, b"local bytes").unwrap();
                let mut local = snapshot(NodeId([1; 32]), Some(SessionId::new()));
                let start = local.draft.text.find("@资料").unwrap();
                let range = start..start + "@资料".len();
                local
                    .draft
                    .tokens
                    .push(crate::conversation::live::recovery::Token {
                        range: range.clone(),
                        id: "reference-fixture".into(),
                        text: "@资料".into(),
                        label: "@资料".into(),
                    });
                local.draft.selection = range;
                local
                    .draft
                    .attachments
                    .push(crate::conversation::live::recovery::Attachment {
                        index: 0,
                        value: sailry_protocol::attachment::Attachment {
                            id: sailry_protocol::AttachmentId::new(),
                            spec: sailry_protocol::attachment::Spec {
                                worktree: local.draft.identity.worktree.unwrap(),
                                name: "captured.txt".into(),
                                media_type: "text/plain".into(),
                                size: 14,
                                revision: blake3::hash(b"captured bytes").to_hex().to_string(),
                            },
                        },
                    });
                local.sources = vec![
                    Local::Path(file.clone()),
                    Local::Image {
                        name: "pasted.png".into(),
                        format: ImageFormat::Png,
                        bytes: Arc::from(b"image fixture".as_slice()),
                    },
                ];
                let remote = snapshot(NodeId([2; 32]), None);
                Capture {
                    snapshots: vec![local.clone(), remote.clone()],
                    expected: Arc::new(Mutex::new(None)),
                }
                .save(directory.path())
                .unwrap();
                fs::remove_file(file).unwrap();
                let (_, loaded) = load(directory.path()).unwrap();
                assert_eq!(
                    loaded.len(),
                    2,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    loaded[0].snapshot.draft.identity, local.draft.identity,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    loaded[1].snapshot.draft.identity, remote.draft.identity,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    loaded[0].snapshot.draft.text, local.draft.text,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    loaded[0].snapshot.draft.references, local.draft.references,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    loaded[0].snapshot.draft.attachments, local.draft.attachments,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    loaded[0].snapshot.draft.tokens, local.draft.tokens,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    loaded[0].snapshot.draft.selection, local.draft.selection,
                    "preserves scoped references and attachment bytes"
                );
                match &loaded[0].snapshot.sources[0] {
                    Local::Path(path) => {
                        assert_eq!(
                            path.file_name().unwrap(),
                            "资料.bin",
                            "preserves scoped references and attachment bytes"
                        );
                        assert_eq!(
                            fs::read(path).unwrap(),
                            b"local bytes",
                            "preserves scoped references and attachment bytes"
                        );
                    }
                    _ => panic!("file source expected"),
                }
                match &loaded[0].snapshot.sources[1] {
                    Local::Image { format, bytes, .. } => {
                        assert_eq!(
                            *format,
                            ImageFormat::Png,
                            "preserves scoped references and attachment bytes"
                        );
                        assert_eq!(
                            &**bytes, b"image fixture",
                            "preserves scoped references and attachment bytes"
                        );
                    }
                    _ => panic!("image source expected"),
                }
                fs::write(&loaded[0].marker, b"restored").unwrap();
                let (_, remaining) = load(directory.path()).unwrap();
                assert_eq!(
                    remaining.len(),
                    1,
                    "preserves scoped references and attachment bytes"
                );
                assert_eq!(
                    remaining[0].snapshot.draft.identity.node,
                    NodeId([2; 32]),
                    "preserves scoped references and attachment bytes"
                );
            }) as fn(&str),
        ),
        (
            "preserves_unreadable_or_changed_cache",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let path = directory.path().join("recovery.json");
                fs::write(&path, b"incompatible fixture").unwrap();
                assert!(
                    load(directory.path()).is_err(),
                    "preserves unreadable or changed cache"
                );
                assert!(
                    Capture {
                        snapshots: vec![],
                        expected: Arc::new(Mutex::new(None))
                    }
                    .save(directory.path())
                    .is_err(),
                    "preserves unreadable or changed cache"
                );
                assert_eq!(
                    fs::read(path).unwrap(),
                    b"incompatible fixture",
                    "preserves unreadable or changed cache"
                );
            }),
        ),
        (
            "missing_attachments_preserve_previous_cache",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                Capture {
                    snapshots: vec![],
                    expected: Arc::new(Mutex::new(None)),
                }
                .save(directory.path())
                .unwrap();
                let (expected, _) = load(directory.path()).unwrap();
                let before = fs::read(directory.path().join("recovery.json")).unwrap();
                let mut draft = snapshot(NodeId([3; 32]), None);
                draft
                    .sources
                    .push(Local::Path(directory.path().join("missing.bin")));
                assert!(
                    Capture {
                        snapshots: vec![draft],
                        expected: Arc::new(Mutex::new(expected))
                    }
                    .save(directory.path())
                    .is_err(),
                    "missing attachments preserve previous cache"
                );
                assert_eq!(
                    fs::read(directory.path().join("recovery.json")).unwrap(),
                    before,
                    "missing attachments preserve previous cache"
                );
            }),
        ),
        (
            "draft_retries_preserve_external_revisions",
            (|_scenario: &str| {
                let directory = tempfile::tempdir().unwrap();
                let expected = Arc::new(Mutex::new(None));
                let mut saved = snapshot(NodeId([3; 32]), None);
                for text in ["first saved draft", "edited after preparation failed"] {
                    saved.draft.text = text.into();
                    Capture {
                        snapshots: vec![saved.clone()],
                        expected: expected.clone(),
                    }
                    .save(directory.path())
                    .unwrap();
                    assert_eq!(
                        load(directory.path()).unwrap().1[0].snapshot.draft.text,
                        text,
                        "draft retries preserve external revisions"
                    );
                }
                let path = directory.path().join("recovery.json");
                let mut external = fs::read(&path).unwrap();
                external.push(b' ');
                fs::write(&path, &external).unwrap();
                assert!(
                    Capture {
                        snapshots: vec![saved],
                        expected
                    }
                    .save(directory.path())
                    .is_err(),
                    "draft retries preserve external revisions"
                );
                assert_eq!(
                    fs::read(path).unwrap(),
                    external,
                    "draft retries preserve external revisions"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}

#[test]
fn element_chips_keep_their_drafts() {
    let directory = tempfile::tempdir().unwrap();
    let mut draft = snapshot(NodeId([4; 32]), None);
    draft.draft.text = "Fix @PayButton ".into();
    draft.draft.tokens = vec![crate::conversation::live::recovery::Token {
        range: 4..14,
        id: "element:3".into(),
        text: "@PayButton".into(),
        label: "@PayButton".into(),
    }];
    let chip = crate::conversation::live::recovery::Chip {
        key: 3,
        label: "PayButton".into(),
        details: crate::browser::annotation::Details {
            selector: "#pay".into(),
            component: "PayButton · src/Pay.tsx:12".into(),
            page: "Checkout".into(),
        },
        drafts: vec![0, 1],
    };
    draft.draft.chips = vec![chip.clone()];
    draft.sources = vec![
        Local::Image {
            name: "browser-element-0.jpg".into(),
            format: ImageFormat::Jpeg,
            bytes: Arc::from(b"capture".as_slice()),
        },
        Local::Data {
            name: "browser-element-0.json".into(),
            media_type: "application/json",
            bytes: Arc::from(br#"{"kind":"browser_element"}"#.as_slice()),
        },
    ];
    Capture {
        snapshots: vec![draft],
        expected: Arc::new(Mutex::new(None)),
    }
    .save(directory.path())
    .unwrap();
    let (_, loaded) = load(directory.path()).unwrap();
    let restored = &loaded[0].snapshot;
    assert_eq!(restored.draft.chips, vec![chip]);
    assert_eq!(restored.draft.tokens[0].id, "element:3");
    let Local::Path(path) = &restored.sources[1] else {
        panic!("generated context restores as its file")
    };
    assert_eq!(path.file_name().unwrap(), "browser-element-0.json");
    assert_eq!(fs::read(path).unwrap(), br#"{"kind":"browser_element"}"#);
}
