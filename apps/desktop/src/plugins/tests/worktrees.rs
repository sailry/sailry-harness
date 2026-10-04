//! Actual Worktrees package interactions share the local and Link command boundary.
use super::*;
use crate::{plugins::contributions::Registry, preview::Page, shell::Shell};
use sailry_protocol::{Output, Request, WorktreeId, plugin::ui::Intent};

mod fixture;
use fixture::{Fixture as Worktrees, input, invoke, ready};

#[gpui::test]
fn creates_and_selects_registered_checkouts(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Worktrees::new(remote);
        let (shell, mut registry, visual) = fixture.mount(cx);
        invoke(
            &shell,
            &mut registry,
            Intent::Worktrees,
            serde_json::Value::Null,
            visual,
        );
        ready(&registry, visual, "entry-1");
        click(visual, "entry-1");
        ready(&registry, visual, "action-0");
        click(visual, "action-0");
        wait(visual, |cx| {
            fixture.selected(&shell, cx) != Some(fixture.base.session.worktree)
        });
        let linked = visual.update(|_, cx| fixture.selected(&shell, cx).unwrap());
        assert_eq!(
            fixture
                .mutations()
                .iter()
                .filter(|request| matches!(request.command, Command::RegisterWorktree { .. }))
                .count(),
            1
        );
        assert_ne!(linked, fixture.base.session.worktree);

        let path = fixture.base.directory.path().join("created");
        invoke(
            &shell,
            &mut registry,
            Intent::CreateWorktree,
            serde_json::json!({"branch":"main","commit":fixture.head}),
            visual,
        );
        ready(&registry, visual, "live-worktree-form");
        input(visual, "live-worktree-base", "release");
        input(visual, "live-worktree-branch", "invalid branch");
        input(visual, "live-worktree-path", path.to_str().unwrap());
        visual.update(|window, cx| window.clear_notifications(cx));
        click(visual, "live-worktree-submit");
        toast(visual, "Check the branch and absolute path");
        ready(&registry, visual, "live-worktree-form");
        let first = fixture.creates()[0].clone();
        let Command::CreateWorktree {
            path: destination,
            branch,
            commit,
            ..
        } = &first.command
        else {
            panic!("create expected")
        };
        assert_eq!(destination, path.to_str().unwrap());
        assert_eq!(branch, "invalid branch");
        assert_eq!(commit, &fixture.head);
        fixture.move_tag();
        visual.update(|window, cx| window.clear_notifications(cx));
        click(visual, "live-worktree-submit");
        wait(visual, |_| fixture.creates().len() == 2);
        toast(visual, "Check the branch and absolute path");
        ready(&registry, visual, "live-worktree-form");
        assert_eq!(fixture.creates()[1], first);
        assert!(!path.exists());
        input(visual, "live-worktree-branch", "created");
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            path.exists()
                && fixture
                    .selected(&shell, cx)
                    .is_some_and(|tree| tree != linked)
        });
        let creates = fixture.creates();
        assert_ne!(creates[2].id, first.id);
        let Command::CreateWorktree { commit, .. } = &creates[2].command else {
            panic!("create expected")
        };
        assert_eq!(commit, &fixture.current_head());
        assert!(
            creates.iter().all(
                |request| request
                    .plugin
                    .as_ref()
                    .is_some_and(|context| context.package.name == "worktrees"
                        && context.worktree == Some(linked))
            )
        );
        assert_eq!(
            std::fs::read_to_string(path.join("notes.txt")).unwrap(),
            "committed\n"
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn removal_preserves_observed_revision_and_branch(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Worktrees::new(remote);
        let (shell, mut registry, visual) = fixture.mount(cx);
        invoke(
            &shell,
            &mut registry,
            Intent::Worktrees,
            serde_json::Value::Null,
            visual,
        );
        ready(&registry, visual, "entry-1");
        click(visual, "entry-1");
        ready(&registry, visual, "action-0");
        click(visual, "action-1");
        ready(&registry, visual, "worktree-remove-form");
        assert_eq!(
            visual.update(|_, cx| fixture.selected(&shell, cx)),
            fixture.base.binding.worktree
        );
        click(visual, "worktree-remove-cancel");
        assert!(fixture.linked.exists());
        assert!(fixture.removals().is_empty());

        let document = fixture.dirty(&shell, visual);
        invoke(
            &shell,
            &mut registry,
            Intent::Worktrees,
            serde_json::Value::Null,
            visual,
        );
        ready(&registry, visual, "entry-1");
        click(visual, "entry-1");
        ready(&registry, visual, "action-0");
        visual.update(|window, cx| window.clear_notifications(cx));
        click(visual, "action-1");
        toast(visual, "Save changes before removing this worktree");
        assert!(fixture.removals().is_empty());
        assert!(document.read_with(visual, |document, cx| document.has_unsaved(cx)));
        visual.simulate_keystrokes("escape");
        visual.update(|_, cx| document.update(cx, |document, cx| document.discard_all(cx)));

        invoke(
            &shell,
            &mut registry,
            Intent::Worktrees,
            serde_json::Value::Null,
            visual,
        );
        ready(&registry, visual, "entry-1");
        click(visual, "entry-1");
        ready(&registry, visual, "action-0");
        click(visual, "action-1");
        ready(&registry, visual, "worktree-remove-form");
        fixture.advance(&fixture.linked);
        visual.update(|window, cx| window.clear_notifications(cx));
        click(visual, "worktree-remove-confirm");
        toast(visual, "Worktree changed, refresh before removing");
        ready(&registry, visual, "worktree-remove-form");
        let request = fixture.removals()[0].clone();
        let Command::RemoveWorktree {
            expected_head,
            expected_branch,
            ..
        } = &request.command
        else {
            panic!("removal expected")
        };
        assert_eq!(expected_head, &fixture.head);
        assert_eq!(expected_branch, "feature");
        visual.update(|window, cx| window.clear_notifications(cx));
        click(visual, "worktree-remove-confirm");
        wait(visual, |_| fixture.removals().len() == 2);
        toast(visual, "Worktree changed, refresh before removing");
        ready(&registry, visual, "worktree-remove-form");
        assert_eq!(fixture.removals()[1], request);
        assert!(fixture.linked.exists());
        click(visual, "worktree-remove-cancel");

        invoke(
            &shell,
            &mut registry,
            Intent::Worktrees,
            serde_json::Value::Null,
            visual,
        );
        ready(&registry, visual, "entry-1");
        click(visual, "entry-1");
        ready(&registry, visual, "action-0");
        click(visual, "action-1");
        ready(&registry, visual, "worktree-remove-form");
        click(visual, "worktree-remove-confirm");
        wait(visual, |_| !fixture.linked.exists());
        let repository = git2::Repository::open(fixture.root()).unwrap();
        assert!(
            repository
                .find_branch("feature", git2::BranchType::Local)
                .is_ok()
        );
        assert_eq!(
            visual.update(|_, cx| fixture.selected(&shell, cx)),
            fixture.base.binding.worktree
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn close_and_host_change_preserve_captured_target(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Worktrees::new(remote);
        let (shell, mut registry, visual) = fixture.mount(cx);
        let path = fixture.base.directory.path().join("cancelled");
        invoke(
            &shell,
            &mut registry,
            Intent::CreateWorktree,
            serde_json::json!({"branch":"main","commit":fixture.head}),
            visual,
        );
        ready(&registry, visual, "live-worktree-form");
        input(visual, "live-worktree-branch", "cancelled");
        input(visual, "live-worktree-path", path.to_str().unwrap());
        click(visual, "live-worktree-cancel");
        assert!(fixture.creates().is_empty());
        assert!(!path.exists());
        invoke(
            &shell,
            &mut registry,
            Intent::CreateWorktree,
            serde_json::json!({"branch":"main","commit":fixture.head}),
            visual,
        );
        ready(&registry, visual, "live-worktree-form");
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .select(fixture.base.controller.id(), cx)
            })
        });
        wait(visual, |cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .view
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.node == fixture.base.controller.id())
        });
        assert!(visual.debug_bounds("live-worktree-submit").is_none());
        assert!(fixture.creates().is_empty());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
