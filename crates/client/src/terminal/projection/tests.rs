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
fn reopening_resets_sequence() {
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
        Apply::Applied
    );
    assert_eq!(
        projection
            .apply(2, Update::TerminalSnapshot(original))
            .unwrap(),
        Apply::Ignored
    );
    assert_eq!(
        projection.apply(2, frame(&reopened, 1)).unwrap(),
        Apply::Applied
    );
    assert_eq!(
        projection.snapshot().unwrap().info.revision,
        reopened.info.revision
    );
}

#[test]
fn recovers_screen_after_gaps() {
    let initial = snapshot();
    let mut projection = Projection::new(initial.node, initial.info.id, 1);
    assert_eq!(
        projection
            .apply(1, Update::TerminalSnapshot(initial.clone()))
            .unwrap(),
        Apply::Applied
    );
    assert_eq!(
        projection.apply(1, frame(&initial, 4)).unwrap(),
        Apply::Applied
    );
    assert_eq!(
        projection.apply(1, frame(&initial, 4)).unwrap(),
        Apply::Ignored
    );
    assert_eq!(
        projection.apply(1, frame(&initial, 6)).unwrap(),
        Apply::Recover
    );
    assert_eq!(projection.snapshot().unwrap().sequence, 4);
    assert_eq!(
        projection.apply(1, frame(&initial, 5)).unwrap(),
        Apply::Recover
    );
    projection.reconnect(2).unwrap();
    let mut recovered = initial.clone();
    recovered.sequence = 7;
    assert_eq!(
        projection
            .apply(1, Update::TerminalSnapshot(recovered.clone()))
            .unwrap(),
        Apply::Ignored
    );
    assert_eq!(
        projection
            .apply(2, Update::TerminalSnapshot(recovered))
            .unwrap(),
        Apply::Applied
    );
    assert_eq!(
        projection.apply(2, frame(&initial, 8)).unwrap(),
        Apply::Applied
    );
}

#[test]
fn rejects_invalid_frames() {
    let initial = snapshot();
    let mut projection = Projection::new(initial.node, initial.info.id, 1);
    let mut invalid = initial.clone();
    invalid.info.id = TerminalId::new();
    assert_eq!(
        projection
            .apply(1, Update::TerminalSnapshot(invalid))
            .unwrap_err()
            .code,
        ErrorCode::WrongTarget
    );
    let mut invalid = initial.clone();
    invalid.screen.rows[0].spans.push(terminal::Span {
        column: 79,
        columns: 2,
        text: "宽".into(),
        style: terminal::TextStyle::default(),
        hyperlink: None,
    });
    assert_eq!(
        projection
            .apply(1, Update::TerminalSnapshot(invalid))
            .unwrap_err()
            .code,
        ErrorCode::InvalidRequest
    );
    assert!(projection.snapshot().is_none());
}

#[test]
fn applies_scrollback_patch() {
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
    assert_eq!(screen.scrollback.len(), 2);
    assert!(screen.bracketed_paste);
    assert!(screen.mouse_tracking);
    assert!(screen.alternate);
    assert_eq!(screen.graphics, initial.screen.graphics);
    projection.reconnect(2).unwrap();
    initial.sequence = 4;
    projection
        .apply(2, Update::TerminalSnapshot(initial.clone()))
        .unwrap();
    assert_eq!(
        projection.snapshot().unwrap().screen.graphics,
        initial.screen.graphics
    );
    let mut removed = initial.clone();
    removed.screen.graphics = Default::default();
    assert_eq!(
        projection.apply(2, frame(&removed, 5)).unwrap(),
        Apply::Applied
    );
    assert!(
        projection
            .snapshot()
            .unwrap()
            .screen
            .graphics
            .images
            .is_empty()
    );
}

#[test]
fn rejects_invalid_images() {
    let mut initial = snapshot();
    initial.screen.graphics.images.push(terminal::Image {
        id: 1,
        generation: 1,
        width: 9000,
        height: 9000,
        png: "".into(),
    });
    assert!(validate(&initial.screen).is_err());
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
    assert!(validate(&initial.screen).is_err());
}
