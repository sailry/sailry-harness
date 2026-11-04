use super::*;

fn snapshot() -> Snapshot {
    let color = terminal::Rgb {
        red: 1,
        green: 2,
        blue: 3,
    };
    Snapshot {
        node: NodeId([1; 32]),
        info: terminal::Info {
            id: TerminalId::new(),
            worktree: Some(WorktreeId::new()),
            access: Some(sailry_protocol::process::Access::Workspace),
            ssh: None,
            tool: None,
            status: terminal::Status::Running,
            activity: None,
            title: None,
            directory: None,
            owner: Some(NodeId([1; 32])),
            revision: 1,
        },
        sequence: 3,
        screen: Screen {
            columns: 80,
            rows: vec![terminal::Line::default(); 24],
            scrollback: vec![terminal::Line::default()],
            cursor: None,
            foreground: color,
            background: color,
            cursor_color: color,
            bracketed_paste: false,
            mouse_tracking: false,
            alternate: false,
            features: Default::default(),
            graphics: Default::default(),
        },
    }
}

fn frame(snapshot: &Snapshot, sequence: u64) -> Update {
    Update::TerminalFrame(terminal::Frame {
        node: snapshot.node,
        info: snapshot.info.clone(),
        sequence,
        screen: ScreenUpdate::Replace {
            screen: snapshot.screen.clone(),
        },
    })
}

#[test]
fn recovers_sequences_and_rejects_invalid_frames() {
    for (case_name, run) in [
        (
            "reopening_resets_sequence",
            (|case_name| {
                let original = snapshot();
                let mut projection = Projection::new(original.node, original.info.id, 1);
                projection
                    .apply(1, Update::TerminalSnapshot(original.clone()))
                    .unwrap();
                projection.reconnect(2).unwrap();
                let mut reopened = original.clone();
                reopened.sequence = 0;
                reopened.info.revision += 2;
                assert_eq!(
                    projection
                        .apply(2, Update::TerminalSnapshot(reopened.clone()))
                        .unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection
                        .apply(2, Update::TerminalSnapshot(original))
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(2, frame(&reopened, 1)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.snapshot().unwrap().info.revision,
                    reopened.info.revision,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "recovers_screen_after_gaps",
            (|case_name| {
                let initial = snapshot();
                let mut projection = Projection::new(initial.node, initial.info.id, 1);
                assert_eq!(
                    projection
                        .apply(1, Update::TerminalSnapshot(initial.clone()))
                        .unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, frame(&initial, 4)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, frame(&initial, 4)).unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(1, frame(&initial, 6)).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                assert_eq!(projection.snapshot().unwrap().sequence, 4, "{case_name}");
                assert_eq!(
                    projection.apply(1, frame(&initial, 5)).unwrap(),
                    Apply::Recover,
                    "{case_name}"
                );
                projection.reconnect(2).unwrap();
                let mut recovered = initial.clone();
                recovered.sequence = 7;
                assert_eq!(
                    projection
                        .apply(1, Update::TerminalSnapshot(recovered.clone()))
                        .unwrap(),
                    Apply::Ignored,
                    "{case_name}"
                );
                assert_eq!(
                    projection
                        .apply(2, Update::TerminalSnapshot(recovered))
                        .unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert_eq!(
                    projection.apply(2, frame(&initial, 8)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "rejects_foreign_targets",
            (|case_name| {
                let initial = snapshot();
                let mut projection = Projection::new(initial.node, initial.info.id, 1);
                let mut invalid = initial.clone();
                invalid.info.id = TerminalId::new();
                assert_eq!(
                    projection
                        .apply(1, Update::TerminalSnapshot(invalid))
                        .unwrap_err()
                        .code,
                    ErrorCode::WrongTarget,
                    "{case_name}"
                );
                let mut invalid = initial.clone();
                invalid.node = NodeId([2; 32]);
                invalid.screen.columns = 0;
                assert_eq!(
                    projection.apply(1, frame(&invalid, 4)).unwrap_err().code,
                    ErrorCode::WrongTarget,
                    "{case_name}"
                );
                assert!(projection.snapshot().is_none(), "{case_name}");
            }) as fn(&str),
        ),
        (
            "recovers_malformed_frames",
            (|case_name| {
                let initial = snapshot();
                for history in [false, true] {
                    let mut invalid = initial.clone();
                    invalid.sequence += 1;
                    let lines = if history {
                        &mut invalid.screen.scrollback
                    } else {
                        &mut invalid.screen.rows
                    };
                    lines[0].spans.push(terminal::Span {
                        column: 79,
                        columns: 2,
                        text: "宽".into(),
                        style: Default::default(),
                        hyperlink: None,
                    });
                    let screen = &invalid.screen;
                    let patch = terminal::Patch {
                        dropped_scrollback_rows: initial.screen.scrollback.len() as u32,
                        appended_scrollback: screen.scrollback.clone(),
                        columns: screen.columns,
                        rows: screen.rows.clone(),
                        cursor: screen.cursor,
                        foreground: screen.foreground,
                        background: screen.background,
                        cursor_color: screen.cursor_color,
                        bracketed_paste: screen.bracketed_paste,
                        mouse_tracking: screen.mouse_tracking,
                        alternate: screen.alternate,
                        features: screen.features.clone(),
                        graphics: Some(screen.graphics.clone()),
                    };
                    for update in [
                        Update::TerminalSnapshot(invalid.clone()),
                        frame(&invalid, invalid.sequence),
                        Update::TerminalFrame(terminal::Frame {
                            node: invalid.node,
                            info: invalid.info.clone(),
                            sequence: invalid.sequence,
                            screen: ScreenUpdate::Patch { patch },
                        }),
                    ] {
                        let mut projection = Projection::new(initial.node, initial.info.id, 1);
                        projection
                            .apply(1, Update::TerminalSnapshot(initial.clone()))
                            .unwrap();
                        assert_eq!(
                            projection.apply(1, update).unwrap(),
                            Apply::Recover,
                            "{case_name}"
                        );
                        assert_eq!(projection.snapshot(), Some(&initial), "{case_name}");
                        assert_eq!(
                            projection.apply(1, frame(&initial, 4)).unwrap(),
                            Apply::Recover,
                            "{case_name}"
                        );
                        projection.reconnect(2).unwrap();
                        let mut recovered = initial.clone();
                        recovered.sequence = 5;
                        assert_eq!(
                            projection
                                .apply(2, Update::TerminalSnapshot(recovered.clone()))
                                .unwrap(),
                            Apply::Applied,
                            "{case_name}"
                        );
                        assert_eq!(projection.snapshot(), Some(&recovered), "{case_name}");
                        assert_eq!(
                            projection.apply(2, frame(&recovered, 6)).unwrap(),
                            Apply::Applied,
                            "{case_name}"
                        );
                    }
                    let mut projection = Projection::new(initial.node, initial.info.id, 1);
                    assert_eq!(
                        projection
                            .apply(1, Update::TerminalSnapshot(invalid))
                            .unwrap(),
                        Apply::Recover,
                        "{case_name}"
                    );
                    assert!(projection.snapshot().is_none(), "{case_name}");
                }
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn preserves_and_validates_graphics() {
    for (case_name, run) in [
        (
            "applies_scrollback_patch",
            (|case_name| {
                let mut initial = snapshot();
                initial.screen.graphics.images.push(terminal::Image {
        id: 1,
        generation: 1,
        width: 1,
        height: 1,
        png: "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==".into(),
    });
                let mut projection = Projection::new(initial.node, initial.info.id, 1);
                projection
                    .apply(1, Update::TerminalSnapshot(initial.clone()))
                    .unwrap();
                let screen = &initial.screen;
                let patch = terminal::Patch {
                    dropped_scrollback_rows: 1,
                    appended_scrollback: vec![terminal::Line::default(); 2],
                    columns: screen.columns,
                    rows: screen.rows.clone(),
                    cursor: None,
                    foreground: screen.foreground,
                    background: screen.background,
                    cursor_color: screen.cursor_color,
                    bracketed_paste: true,
                    mouse_tracking: true,
                    alternate: true,
                    features: Default::default(),
                    graphics: Default::default(),
                };
                projection
                    .apply(
                        1,
                        Update::TerminalFrame(terminal::Frame {
                            node: initial.node,
                            info: initial.info.clone(),
                            sequence: 4,
                            screen: ScreenUpdate::Patch { patch },
                        }),
                    )
                    .unwrap();
                let screen = &projection.snapshot().unwrap().screen;
                assert_eq!(screen.scrollback.len(), 2, "{case_name}");
                assert!(screen.bracketed_paste, "{case_name}");
                assert!(screen.mouse_tracking, "{case_name}");
                assert!(screen.alternate, "{case_name}");
                assert_eq!(screen.graphics, initial.screen.graphics, "{case_name}");
                projection.reconnect(2).unwrap();
                initial.sequence = 4;
                projection
                    .apply(2, Update::TerminalSnapshot(initial.clone()))
                    .unwrap();
                assert_eq!(
                    projection.snapshot().unwrap().screen.graphics,
                    initial.screen.graphics,
                    "{case_name}"
                );
                let mut removed = initial.clone();
                removed.screen.graphics = Default::default();
                assert_eq!(
                    projection.apply(2, frame(&removed, 5)).unwrap(),
                    Apply::Applied,
                    "{case_name}"
                );
                assert!(
                    projection
                        .snapshot()
                        .unwrap()
                        .screen
                        .graphics
                        .images
                        .is_empty(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "rejects_invalid_images",
            (|case_name| {
                let mut initial = snapshot();
                initial.screen.graphics.images.push(terminal::Image {
                    id: 1,
                    generation: 1,
                    width: 9000,
                    height: 9000,
                    png: "".into(),
                });
                assert!(initial.screen.validate().is_err(), "{case_name}");
                initial.screen.graphics.images.clear();
                initial
                    .screen
                    .graphics
                    .placements
                    .push(terminal::Placement {
                        image: 1,
                        id: 1,
                        column: 0,
                        row: 0,
                        offset: [0, 0],
                        size: [1, 1],
                        source: [0, 0, 1, 1],
                        tile: None,
                        cell: [8, 16],
                        z: 0,
                    });
                assert!(initial.screen.validate().is_err(), "{case_name}");
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}
