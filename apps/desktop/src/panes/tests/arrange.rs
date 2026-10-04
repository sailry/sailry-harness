use super::*;

#[gpui::test]
fn append_and_restore(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let targets: Vec<_> = (0..=LIMIT).map(|index| session(index as u8)).collect();
    for target in &targets {
        open(&workspaces, *target, visual);
    }
    assert!(!workspaces.read_with(visual, |spaces, cx| {
        spaces.can_append(targets[0], targets[1], cx)
    }));
    assert!(drop_on(
        &workspaces,
        targets[1],
        targets[0],
        Some(Placement::Left),
        visual
    ));
    let mut expected = vec![targets[1], targets[0]];
    for target in &targets[2..LIMIT] {
        visual.update(|window, cx| {
            workspaces.update(cx, |spaces, cx| {
                let pane = spaces.panes[target].entity_id();
                assert!(spaces.append(targets[0], *target, window, cx));
                assert_eq!(spaces.panes[target].entity_id(), pane);
                expected.push(*target);
                assert_eq!(spaces.ordered_members(targets[0], cx), expected);
                assert!(!spaces.can_append(targets[0], *target, cx));
                spaces.saved(cx).validate().unwrap();
            })
        });
    }
    let saved = visual.update(|window, cx| {
        workspaces.update(cx, |spaces, cx| {
            assert!(!spaces.append(targets[0], targets[LIMIT], window, cx));
            assert!(!spaces.can_append(targets[0], Target::Draft, cx));
            assert_eq!(spaces.ordered_members(targets[LIMIT], cx), [targets[LIMIT]]);
            spaces.saved(cx)
        })
    });
    let restored = visual.update(|window, cx| {
        let restored = cx.new(|_| Workspaces::new());
        restored.update(cx, |spaces, cx| spaces.restore(&saved, window, cx));
        restored
    });
    restored.read_with(visual, |spaces, cx| {
        assert_eq!(spaces.ordered_members(targets[0], cx), expected);
        spaces.saved(cx).validate().unwrap();
    });
    visual.update(|window, cx| {
        restored.update(cx, |spaces, cx| {
            spaces.detach(targets[2], window, cx);
            expected.retain(|target| *target != targets[2]);
            assert_eq!(spaces.ordered_members(targets[0], cx), expected);
            spaces.saved(cx).validate().unwrap();
        })
    });
}

#[gpui::test]
fn narrow_layout(cx: &mut TestAppContext) {
    let (workspaces, visual) = mount(cx);
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(500.), px(1100.)));
    let targets = [session(0), session(1), session(2)];
    for target in targets {
        open(&workspaces, target, visual);
    }
    assert!(drop_on(
        &workspaces,
        targets[1],
        targets[0],
        Some(Placement::Bottom),
        visual
    ));
    visual.update(|window, cx| {
        workspaces.update(cx, |spaces, cx| spaces.focus(targets[0], cx));
        window.draw(cx).clear(cx);
        workspaces.update(cx, |spaces, cx| {
            assert!(spaces.append(targets[0], targets[2], window, cx))
        });
    });
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    workspaces.read_with(visual, |spaces, cx| {
        let bounds = targets.map(|target| spaces.panes[&target].read(cx).bounds.get());
        assert!(bounds[0].bottom() < bounds[1].top());
        assert!(bounds[1].bottom() < bounds[2].top());
        assert_eq!(bounds[0].left(), bounds[2].left());
        assert_eq!(bounds[0].size.width, bounds[2].size.width);
    });
}
