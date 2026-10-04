use super::*;
use std::{cell::RefCell, rc::Rc};

#[gpui::test]
fn bound_pages_and_unavailable_targets(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        let routes = Rc::new(RefCell::new(Vec::new()));
        visual.update(|_, cx| {
            let routes = routes.clone();
            cx.subscribe(&view, move |_, event: &Event, _| match event {
                Event::HostPage(node) => routes.borrow_mut().push((*node, None)),
                Event::ProjectPage(node, project) => {
                    routes.borrow_mut().push((*node, Some(*project)))
                }
                _ => {}
            })
            .detach();
        });
        for target in [
            Target::Host,
            Target::Project,
            Target::Worktree,
            Target::Skill {
                package: "reports".into(),
                name: "analysis".into(),
            },
            Target::Agent(sailry_protocol::role::Reference {
                id: sailry_protocol::RoleId::new(),
                revision: 1,
            }),
            Target::Ssh(sailry_protocol::SshId::new()),
            Target::Database {
                connection: sailry_protocol::DatabaseId::new(),
                database: None,
                table: None,
            },
            Target::Session(SessionId::new()),
        ] {
            visual.update(|window, cx| {
                window.clear_notifications(cx);
                view.update(cx, |view, cx| {
                    view.references.selected.clear();
                    let token = view.remember_reference(Reference {
                        target: target.clone(),
                        label: "Selected".into(),
                    });
                    view.input.update(cx, |input, cx| {
                        input.set_value("", window, cx);
                        input.replace_with_token(token, window, cx).unwrap();
                    });
                });
            });
            inline::click_token(visual, &view, "@Selected");
            visual.run_until_parked();
            visual.update(|window, cx| {
                if matches!(target, Target::Worktree) {
                    assert!(window.has_active_dialog(cx));
                    window.draw(cx).clear(cx);
                    assert!(window.notifications(cx).is_empty());
                } else if matches!(target, Target::Host | Target::Project) {
                    assert!(!window.has_active_dialog(cx));
                    assert!(window.notifications(cx).is_empty());
                } else {
                    assert!(!window.has_active_dialog(cx));
                    assert_eq!(
                        crate::feedback::tests::summary(window, cx),
                        tr(if matches!(target, Target::Skill { .. }) {
                            "reference_stale"
                        } else {
                            "reference_unavailable"
                        })
                    );
                }
            });
            if matches!(target, Target::Worktree) {
                assert!(visual.debug_bounds("details-content").is_some());
                visual.update(|window, cx| window.close_dialog(cx));
            }
        }
        assert_eq!(
            *routes.borrow(),
            vec![
                (fixture.binding.client.target(), None),
                (fixture.binding.client.target(), fixture.binding.project),
            ]
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
