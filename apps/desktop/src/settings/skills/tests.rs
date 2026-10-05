use super::*;
use crate::settings::Section;
use core::prelude::v1::test;
use sailry_protocol::plugin::skills::Source;
use std::sync::atomic::Ordering;

use crate::settings::plugins::test_support as fixture;
mod source;
use fixture::{Fixture, draw, init, input, menu, shown, tap, wait};

fn ready(owner: &Entity<Workspace>, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        owner
            .read(cx)
            .plugin_catalog
            .metadata
            .as_ref()
            .unwrap()
            .read(cx)
            .settled()
    });
}

#[gpui::test]
fn discovery_preserves_progress_and_responsive_rows(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let source = source::Server::with_description(
            &"Inspect project changes and explain the relevant behavior with focused examples "
                .repeat(5),
        );
        let fixture = Fixture::with_skill_source(remote, &source.endpoint);
        let (owner, visual) = fixture.mount(cx);
        owner.update(visual, |owner, cx| owner.select(Section::Skills, cx));
        ready(&owner, visual);
        tap(visual, "skills-install");
        input(visual, "skill-source-0", "fixture/skills");
        input(visual, "skill-source-1", "main");
        draw(visual);
        let before = visual.debug_bounds("skill-install-editor").unwrap();
        fixture.transport.catalog_hold.store(1, Ordering::SeqCst);
        tap(visual, "skill-discover");
        wait(visual, |_| fixture.transport.catalog_entered.is_cancelled());
        assert_eq!(
            visual
                .debug_bounds("skill-install-editor")
                .unwrap()
                .size
                .height,
            before.size.height
        );
        assert!(
            visual
                .debug_bounds("settings-group-skills_candidates")
                .is_none()
        );
        tap(visual, "skill-discover");
        tap(visual, "skill-install-submit");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        fixture.transport.release.cancel();
        shown(visual, "skill-candidate-1", true);
        let mut heights = Vec::new();
        for width in [480., 1280.] {
            visual.simulate_resize(size(px(width), px(820.)));
            draw(visual);
            let row = visual.debug_bounds("skill-result-0").unwrap();
            let text = visual.debug_bounds("skill-result-0-content").unwrap();
            let control = visual.debug_bounds("skill-candidate-0").unwrap();
            let title = visual.debug_bounds("skill-title-0").unwrap();
            let description = visual.debug_bounds("skill-description-0").unwrap();
            let divider = visual
                .debug_bounds("settings-divider-skills_candidates-1")
                .unwrap();
            assert!(control.left() > text.right());
            assert!(control.right() <= row.right());
            assert_eq!(title.left(), description.left());
            assert!(description.top() >= title.bottom());
            assert!(description.right() <= text.right());
            assert_eq!(divider.size.width, row.size.width);
            heights.push(description.size.height);
        }
        assert!(heights[0] > heights[1]);
        tap(visual, "skill-candidate-0");
        tap(visual, "skill-candidate-0");
        tap(visual, "skill-install-submit");
        assert!(fixture.transport.requests.lock().unwrap().is_empty());
        fixture.close(visual);
    }
}

fn selector(kind: &str, name: &str, skill: &str) -> &'static str {
    Box::leak(format!("skill-{kind}-{name}:{skill}").into_boxed_str())
}

fn absent_on_other(fixture: &Fixture, name: &str) {
    assert!(fixture.runtime.block_on(async {
        let client = sailry_client::Client::new(fixture.other.local());
        matches!(client.execute(client.prepare(Command::ReadPlugin { name: name.into() })).await,
            Err(error) if error.code == ErrorCode::NotFound)
    }));
}

#[gpui::test]
fn cards_share_columns(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let source = source::Server::new();
        let fixture = Fixture::with_skill_source(remote, &source.endpoint);
        let Output::SkillDiscovery(discovery) = fixture.execute(Command::DiscoverSkills {
            source: Source {
                repository: "fixture/skills".into(),
                git_ref: Some("main".into()),
                path: None,
            },
        }) else {
            panic!("skill discovery expected")
        };
        for candidate in &discovery.skills {
            fixture.execute(Command::InstallSkill {
                source: discovery.source.clone(),
                path: candidate.path.clone(),
                name: candidate.name.clone(),
                expected_revision: 0,
            });
        }
        let (owner, visual) = fixture.mount(cx);
        owner.update(visual, |owner, cx| owner.select(Section::Skills, cx));
        ready(&owner, visual);
        draw(visual);
        let cards: Vec<_> = discovery
            .skills
            .iter()
            .map(|skill| {
                visual
                    .debug_bounds(selector("card", &skill.name, &skill.skill.name))
                    .unwrap()
            })
            .collect();
        assert_eq!(cards[0].top(), cards[1].top());
        assert!((cards[0].size.width - cards[1].size.width).abs() < px(1.));
        input(visual, "skills-search", &discovery.skills[0].skill.name);
        let filtered = visual
            .debug_bounds(selector(
                "card",
                &discovery.skills[0].name,
                &discovery.skills[0].skill.name,
            ))
            .unwrap();
        assert!((filtered.size.width - cards[0].size.width).abs() < px(1.));
        fixture.close(visual);
    }
}

#[gpui::test]
fn selection_and_captured_target(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let source = source::Server::new();
        let fixture = Fixture::with_skill_source(remote, &source.endpoint);
        let Output::SkillDiscovery(discovery) = fixture.execute(Command::DiscoverSkills {
            source: Source {
                repository: "fixture/skills".into(),
                git_ref: Some("main".into()),
                path: None,
            },
        }) else {
            panic!("skill discovery expected")
        };
        assert_eq!(discovery.skills.len(), 2);
        let candidate = &discovery.skills[0];
        let card = selector("card", &candidate.name, &candidate.skill.name);
        let toggle = selector("toggle", &candidate.name, &candidate.skill.name);
        let menu_id = selector("menu", &candidate.name, &candidate.skill.name);
        let (owner, visual) = fixture.mount(cx);
        owner.update(visual, |owner, cx| owner.select(Section::Skills, cx));
        ready(&owner, visual);
        tap(visual, "skills-install");
        input(visual, "skill-source-0", "fixture/skills");
        input(visual, "skill-source-1", "main");
        tap(visual, "skill-discover");
        shown(visual, "skill-candidate-1", true);
        tap(visual, "skill-candidate-0");
        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, "skill-install-submit");
        wait(visual, |_| fixture.plugins().len() == 1);
        crate::feedback::tests::shown(visual);
        let original = *fixture.transport.requests.lock().unwrap().last().unwrap();
        fixture.bind(&owner, visual, true);
        tap(visual, "skill-install-submit");
        shown(visual, "skill-install-editor", false);
        assert_eq!(
            *fixture.transport.requests.lock().unwrap().last().unwrap(),
            original
        );
        assert_eq!(fixture.plugins().len(), 1);
        absent_on_other(&fixture, &candidate.name);
        fixture.bind(&owner, visual, false);
        ready(&owner, visual);
        shown(visual, card, true);
        tap(
            visual,
            selector("details", &candidate.name, &candidate.skill.name),
        );
        shown(visual, "skill-details", true);
        shown(visual, "skill-info-skills_repository", true);
        visual.simulate_keystrokes("escape");
        shown(visual, "skill-details", false);
        input(visual, "skills-search", "missing skill");
        shown(visual, card, false);
        shown(visual, "empty-skills_no_matches", true);
        shown(visual, "settings-group-skills_installed", true);
        tap(visual, "skills-search");
        visual.simulate_keystrokes("secondary-a backspace");
        assert!(owner.read_with(visual, |owner, cx| {
            owner.skill_state.query.read(cx).value().is_empty()
        }));
        shown(visual, card, true);

        fixture.transport.mode.store(1, Ordering::SeqCst);
        tap(visual, toggle);
        shown(visual, "skills-retry", true);
        assert!(!fixture.plugins()[0].enabled);
        let original = *fixture.transport.requests.lock().unwrap().last().unwrap();
        fixture.bind(&owner, visual, true);
        shown(visual, card, false);
        tap(visual, "skills-retry");
        shown(visual, "skills-retry", false);
        assert_eq!(
            *fixture.transport.requests.lock().unwrap().last().unwrap(),
            original
        );
        absent_on_other(&fixture, &candidate.name);
        fixture.bind(&owner, visual, false);
        ready(&owner, visual);
        shown(visual, card, true);
        tap(visual, toggle);
        wait(visual, |_| fixture.plugins()[0].enabled);
        wait(visual, |cx| owner.read(cx).skill_state.action.is_none());
        ready(&owner, visual);
        menu(visual, menu_id, 0);
        crate::prompts::tests::answer(visual, "plugins_uninstall");
        wait(visual, |_| fixture.plugins().is_empty());
        shown(visual, card, false);
        fixture.close(visual);
    }
}

#[gpui::test]
fn bundled_package_controls(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::new(false);
    fixture.package("1.0.0");
    fixture.execute(Command::InstallPlugin {
        worktree: fixture.worktree,
        path: "package".into(),
        name: "example".into(),
        expected_revision: 0,
    });
    let (owner, visual) = fixture.mount(cx);
    owner.update(visual, |owner, cx| owner.select(Section::Skills, cx));
    ready(&owner, visual);
    shown(visual, "skill-card-example:analysis", false);
    assert!(
        visual
            .debug_bounds("skill-toggle-example:analysis")
            .is_none()
    );
    assert!(
        visual
            .debug_bounds("skill-remove-example:analysis")
            .is_none()
    );
    shown(visual, "empty-plugins_no_skills", true);
    assert!(fixture.plugins()[0].enabled);
    fixture.close(visual);
}
