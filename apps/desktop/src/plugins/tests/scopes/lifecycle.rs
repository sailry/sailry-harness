use super::*;
use crate::plugins::contributions::bridge::Bridge;
use sailry_protocol::{Output, Worktree, plugin::desktop::Surface};
use std::path::Path;

fn checkout(fixture: &Fixture) -> (Worktree, String) {
    let repository = git2::Repository::open(fixture.directory.path().join("project")).unwrap();
    repository.set_head("refs/heads/main").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("notes.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let head = repository
        .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
        .unwrap()
        .to_string();
    let Output::Worktree(worktree) = fixture.execute(Command::CreateWorktree {
        project: fixture.binding.project.unwrap(),
        path: fixture
            .directory
            .path()
            .join("linked")
            .to_str()
            .unwrap()
            .into(),
        branch: "linked".into(),
        commit: head.clone(),
    }) else {
        panic!("worktree expected");
    };
    assert!(!worktree.main);
    (worktree, head)
}

fn git(fixture: &Fixture) -> sailry_protocol::plugin::Info {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin { name: "git".into() }) else {
        panic!("Git package expected");
    };
    info
}

fn remove(fixture: &Fixture, worktree: &Worktree, head: String) {
    assert_eq!(
        fixture.execute(Command::RemoveWorktree {
            worktree: worktree.id,
            expected_head: head,
            expected_branch: "linked".into(),
        }),
        Output::WorktreeRemoved { id: worktree.id },
    );
    assert!(!Path::new(&worktree.path).exists());
}

#[track_caller]
fn released(panel: &Entity<Panel>, worktree: &Worktree, visual: &mut VisualTestContext) {
    wait(visual, |cx| {
        let panel = panel.read(cx);
        panel.connected
            && panel.snapshot.borrow().as_ref().is_some_and(|snapshot| {
                !snapshot.worktrees.iter().any(|tree| tree.id == worktree.id)
            })
    });
    panel.read_with(visual, |panel, _| {
        assert_eq!(panel.binding.worktree, Some(worktree.id));
        assert!(!panel.loading);
        assert!(panel.mounted.is_none());
        assert!(panel.error.is_none());
    });
    assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
}

#[track_caller]
fn reopen(
    panel: &Entity<Panel>,
    package: &sailry_protocol::plugin::Reference,
    visual: &mut VisualTestContext,
) {
    visual.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            assert!(!panel.available(package));
            assert!(!panel.ready_for(package, cx));
            panel.enter(package.clone(), window, cx);
            panel.open(package.clone(), window, cx);
            panel.restore(window, cx);
            assert!(!panel.loading);
            assert!(panel.mounted.is_none());
            assert!(panel.error.is_none());
        });
    });
    wait(visual, |_| true);
    assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
}

struct Retained {
    panels: Vec<Entity<Panel>>,
    visible: bool,
}

impl Render for Retained {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex().size_full().children(
            self.panels
                .iter()
                .filter(|_| self.visible)
                .map(|panel| div().w_1_2().h_full().child(panel.clone())),
        )
    }
}

#[gpui::test]
fn removal_closes_hidden_controllers(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (worktree, head) = checkout(&fixture);
        let info = git(&fixture);
        let package = info.summary.reference();
        let bridge = Bridge::new(info.extension.unwrap().ui);
        let mut binding = fixture.binding.clone();
        binding.worktree = Some(worktree.id);
        binding.branch = "linked".into();
        let mut panels = Vec::new();
        let mut retained = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            panels.push(cx.new(|cx| Panel::workspace(binding.clone(), true, cx)));
            panels.push(cx.new(|cx| {
                Panel::contributions(binding, None, Surface::Project, bridge.clone(), None, cx)
            }));
            let content = cx.new(|cx| {
                for panel in &panels {
                    cx.observe(panel, |_, _, cx| cx.notify()).detach();
                    Panel::observe_notifications(panel, window, cx);
                }
                Retained {
                    panels: panels.clone(),
                    visible: true,
                }
            });
            retained = Some(content.clone());
            Root::new(content, window, cx)
        });
        wait(visual, |cx| {
            panels
                .iter()
                .all(|panel| panel.read(cx).ready_for(&package, cx))
        });
        visual.update(|window, cx| {
            for panel in &panels {
                panel.update(cx, |panel, cx| panel.open(package.clone(), window, cx));
            }
        });
        wait(visual, |cx| {
            panels.iter().all(|panel| panel.read(cx).resource_active())
                && snapshot(&panels[0], cx).contains("git_no_changes")
                && bridge.ready()
                && [Surface::Workspace, Surface::Project].into_iter().all(|surface| {
                    fixture.transport.requests.lock().unwrap().iter().any(|request| {
                        matches!(request.command, Command::ListGitBranches { worktree: id } if id == worktree.id)
                            && request.plugin.as_ref().is_some_and(|context| context.surface == surface)
                    })
                })
        });
        let hosts: Vec<_> = panels
            .iter()
            .map(|panel| panel.read_with(visual, |panel, _| panel.mounted.as_ref().unwrap().host()))
            .collect();
        let retained = retained.unwrap();
        visual.update(|_, cx| {
            retained.update(cx, |retained, cx| {
                retained.visible = false;
                cx.notify();
            })
        });
        wait(visual, |_| true);
        assert!(visual.update(|window, cx| window.notifications(cx).is_empty()));
        remove(&fixture, &worktree, head);
        for panel in &panels {
            released(panel, &worktree, visual);
        }
        assert!(hosts.iter().all(|host| host.stop_token().is_cancelled()));
        for panel in &panels {
            reopen(panel, &package, visual);
        }
        visual.update(|window, _| window.remove_window());
        drop(retained);
        drop(hosts);
        drop(panels);
        fixture.close();
    }
}

#[gpui::test]
fn removal_cancels_pending_view_load(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let (worktree, head) = checkout(&fixture);
        let package = git(&fixture).summary.reference();
        let mut binding = fixture.binding.clone();
        binding.worktree = Some(worktree.id);
        let mut panel = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Panel::workspace(binding, true, cx));
            panel = Some(view.clone());
            let content = cx.new(|cx| {
                cx.observe(&view, |_, _, cx| cx.notify()).detach();
                Panel::observe_notifications(&view, window, cx);
                Retained {
                    panels: vec![view],
                    visible: false,
                }
            });
            Root::new(content, window, cx)
        });
        let panel = panel.unwrap();
        wait(visual, |cx| panel.read(cx).ready_for(&package, cx));
        *fixture.transport.view_target.lock().unwrap() = Some(package.clone());
        fixture.transport.mode.store(2, Ordering::SeqCst);
        visual.update(|window, cx| {
            panel.update(cx, |panel, cx| panel.open(package.clone(), window, cx));
        });
        wait(visual, |_| fixture.transport.entered.is_cancelled());
        assert!(panel.read_with(visual, |panel, _| panel.loading && panel.mounted.is_none()));
        remove(&fixture, &worktree, head);
        released(&panel, &worktree, visual);
        fixture.transport.release.cancel();
        reopen(&panel, &package, visual);
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 1);
        released(&panel, &worktree, visual);
        visual.update(|window, _| window.remove_window());
        drop(panel);
        fixture.close();
    }
}
