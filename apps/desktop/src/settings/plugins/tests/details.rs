use super::*;

#[gpui::test]
fn scrolls_long_skill_details(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        for name in ["analysis", "writing"] {
            let directory = fixture
                .directory
                .path()
                .join("project/package/skills")
                .join(name);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("SKILL.md"), format!("---\nname: {name}\ndescription: {}\nallowed-tools: [Read, 'Bash(curl:*)']\n---\nInstructions\n", "Inspect project changes and explain their impact. ".repeat(20))).unwrap();
        }
        fixture.execute(Command::InstallPlugin {
            worktree: fixture.worktree,
            path: "package".into(),
            name: "example".into(),
            expected_revision: 0,
        });
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugin-details-example");
        shown(visual, "plugin-info-version", true);
        shown(visual, "settings-heading-skills_details", true);
        shown(visual, "settings-heading-skills_installed", false);
        let dialog = visual.debug_bounds("dialog-0").unwrap();
        let before = visual.debug_bounds("plugin-info-version").unwrap();
        assert!(before.bottom() > dialog.bottom());
        visual.simulate_event(ScrollWheelEvent {
            position: dialog.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-2000.))),
            touch_phase: TouchPhase::Moved,
            modifiers: Modifiers::default(),
        });
        draw(visual);
        let after = visual.debug_bounds("plugin-info-version").unwrap();
        assert!(after.top() < before.top() - px(100.));
        assert!(after.top() >= dialog.top());
        assert!(after.bottom() <= dialog.bottom());
        visual.simulate_keystrokes("escape");
        fixture.close(visual);
    }
}

#[gpui::test]
fn rejects_invalid_skills_before_installing(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.package("1.0.0");
        std::fs::write(
            fixture
                .directory
                .path()
                .join("project/package/skills/analysis/SKILL.md"),
            "not a skill",
        )
        .unwrap();
        let before = fixture.public_packages(false);
        let (_, visual) = fixture.mount(cx);
        tap(visual, "plugins-install");
        select(visual, fixture.directory.path().join("project/package"));
        shown(visual, "plugin-retry", true);
        assert_eq!(fixture.public_packages(false), before);
        assert!(!visual.update(|window, cx| window.notifications(cx).is_empty()));
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("plugins_issue_skill")
        );
        fixture.close(visual);
    }
}
