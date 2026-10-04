use super::super::inline::{
    active, active_content, content, draft_icon, icon, links, marker, token,
};
use super::*;

pub(in crate::conversation::live::references) fn click_token(
    visual: &mut VisualTestContext,
    view: &Entity<View>,
    token: &str,
) {
    // Registered controls can change the toolbar height while dispatching a click.
    // Await their actual readiness before resolving native token hit geometry.
    wait(visual, |cx| {
        let view = view.read(cx);
        view.connected() && view.configured() && view.contributions.read(cx).ready(cx)
    });
    let position = token_position(visual, view, token);
    visual.simulate_mouse_move(position, None, Modifiers::default());
    visual.simulate_click(position, Modifiers::default());
    visual.run_until_parked();
    view.read_with(visual, |view, cx| {
        let input = view.input.read(cx);
        let span = input
            .tokens()
            .iter()
            .find(|span| span.token().text() == token)
            .unwrap();
        assert_eq!(
            input.selected_range(),
            span.range(),
            "native click at {position:?}, token={:?}, input={:?}, text={:?}, busy={}",
            input.range_to_bounds(&span.range()),
            input.input_bounds(),
            input.text_bounds(),
            view.busy()
        );
    });
}

fn token_position(
    visual: &mut VisualTestContext,
    view: &Entity<View>,
    token: &str,
) -> Point<Pixels> {
    // Textarea reflow can schedule another layout after insertion or Undo.
    // Read glyph bounds only after consecutive frames agree, before dispatching input.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut previous = None;
    loop {
        visual.run_until_parked();
        let position = visual.update(|window, cx| {
            window.draw(cx).clear(cx);
            let input = view.read(cx).input.read(cx);
            let source = input.value();
            let start = source.find(token).unwrap();
            let end = start + token.len();
            let bounds = input.range_to_bounds(&(start..end));
            bounds.unwrap().center()
        });
        if previous == Some(position) {
            return position;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "token layout deadline"
        );
        previous = Some(position);
    }
}

#[test]
fn matches_whole_labels_and_excludes_code() {
    let references = vec![
        Reference {
            target: Target::File("a".into()),
            label: "文档 notes.md".into(),
        },
        Reference {
            target: Target::File("b".into()),
            label: "文档 notes.md (2)".into(),
        },
        Reference {
            target: Target::Host,
            label: "Host".into(),
        },
    ];
    let text = "看 @文档 notes.md (2)\n@文档 notes.md， @Host";
    let matched = links(text, &references);
    assert_eq!(
        matched
            .iter()
            .map(|link| &text[link.range.clone()])
            .collect::<Vec<_>>(),
        ["@文档 notes.md (2)", "@文档 notes.md", "@Host"]
    );
    assert_eq!(active(text, &references), references);
    assert!(matched.iter().all(|link| link.icon.is_none()));
    for text in [
        "`@Host`",
        "```\n@Host\n```",
        "[x](file:@Host)",
        "<span title='@Host'></span>",
        "x@Host",
        "@Hostile",
        "@文档 notes.m",
    ] {
        assert!(
            active(text, &references).is_empty(),
            "unexpected reference: {text}"
        );
    }
}

#[test]
fn picker_skills_keep_a_distinct_icon() {
    for name in ["analysis", "pdf", "powerpoint", "word", "excel"] {
        let reference = Reference {
            target: Target::Skill {
                package: "reports".into(),
                name: name.into(),
            },
            label: name.into(),
        };
        assert_eq!(icon(&reference), "reicon:school/book");
        assert_eq!(draft_icon(&reference), None);
    }
    assert_ne!(
        icon(&Reference {
            target: Target::Plugin("reports".into()),
            label: "Reports".into(),
        }),
        "reicon:school/book"
    );
    assert!(
        draft_icon(&Reference {
            target: Target::Plugin("reports".into()),
            label: "Reports".into(),
        })
        .is_none()
    );
    assert!(
        draft_icon(&Reference {
            target: Target::File("notes.md".into()),
            label: "Notes".into(),
        })
        .is_some()
    );
}

#[test]
fn markers_keep_text_and_targets_without_icons() {
    for target in [
        Target::File("notes.md".into()),
        Target::Directory("src".into()),
        Target::Plugin("reports".into()),
        Target::Skill {
            package: "reports".into(),
            name: "analysis".into(),
        },
        Target::Session(SessionId::new()),
    ] {
        let reference = Reference {
            target,
            label: "Selected content".into(),
        };
        let text = marker(&reference);
        let matched = links(&text, std::slice::from_ref(&reference));
        assert_eq!(matched.len(), 1);
        assert_eq!(&text[matched[0].range.clone()], "@Selected content");
        assert_eq!(matched[0].id, text);
        assert!(matched[0].icon.is_none());
        assert_eq!(active(&text, std::slice::from_ref(&reference)), [reference]);
    }
}

#[test]
fn native_identity_does_not_admit_matching_plain_text() {
    use gpui_kit::base::input::InputContent;
    let file = Reference {
        label: "Same".into(),
        target: Target::File("notes.md".into()),
    };
    let skill = Reference {
        label: "Same".into(),
        target: Target::Skill {
            package: "reports".into(),
            name: "analysis".into(),
        },
    };
    let catalog = vec![file.clone(), skill.clone()];
    let text = "@Same @Same";
    let plain = InputContent::new(text);
    assert!(active_content(&plain, &catalog).is_empty());
    let native = plain.with_token(0..5, token(&skill)).unwrap();
    assert_eq!(
        active_content(&native, &catalog).as_slice(),
        std::slice::from_ref(&skill)
    );
    assert_ne!(token(&file).id(), token(&skill).id());
    let mut renamed = skill;
    renamed.label = "Renamed".into();
    assert_eq!(token(&renamed).id(), native.tokens()[0].token().id());
    assert_eq!(content("@Same", &[file]).unwrap().text(), "@Same");
}

#[gpui::test]
fn native_tokens_preserve_multiline_source_and_atomic_undo(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("# Raw **Markdown** 🦀\n");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.insert_file_references(vec![("资料 notes.md".into(), false)], window, cx);
                let token = view.remember_reference(Reference {
                    label: "Analysis".into(),
                    target: Target::Skill {
                        package: "reports".into(),
                        name: "analysis".into(),
                    },
                });
                view.input.update(cx, |input, cx| {
                    let range = input.selected_range();
                    input
                        .replace_range_with_token(range, token, window, cx)
                        .unwrap();
                    input.replace(" ", window, cx);
                });
            });
        });
        visual.simulate_keystrokes("shift-enter");
        let original = view.read_with(visual, |view, cx| view.input.read(cx).content());
        assert_eq!(
            original.text(),
            "# Raw **Markdown** 🦀\n@资料 notes.md @Analysis \n"
        );
        assert_eq!(original.tokens().len(), 2);
        let file = original.tokens()[0].clone();
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let end = original.tokens()[1].range().end;
                view.input
                    .update(cx, |input, cx| input.set_selected_range(end..end, cx));
                view.refresh_references(window, cx);
                assert!(
                    !view.references.open,
                    "an atomic token is not a completion query"
                );
            });
        });
        visual.update(|_, cx| {
            let input = view.read(cx).input.clone();
            input.update(cx, |input, cx| {
                // A partial source selection expands to the whole native token.
                input.set_selected_range(file.range().start + 1..file.range().end - 1, cx);
            });
        });
        visual.simulate_keystrokes("backspace");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).tokens().len()),
            1
        );
        assert_eq!(
            view.read_with(visual, |view, cx| view.active_references(cx).len()),
            1
        );
        visual.simulate_keystrokes("cmd-z");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).content()),
            original
        );
        visual.simulate_keystrokes("cmd-a cmd-c");
        assert_eq!(
            visual.read_from_clipboard().unwrap().text().unwrap(),
            original.text().as_str()
        );
        visual.simulate_keystrokes("cmd-v");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            original.text().clone()
        );
        assert!(view.read_with(visual, |view, cx| view.active_references(cx).is_empty()));
        visual.simulate_keystrokes("cmd-z");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).content()),
            original
        );
        visual.update(|window, cx| {
            let snapshot = view.read(cx).snapshot_draft(cx).unwrap().unwrap();
            let selection = view.read(cx).input.read(cx).selected_range();
            view.update(cx, |view, cx| {
                view.references.selected.clear();
                view.input
                    .update(cx, |input, cx| input.set_value("", window, cx));
                assert!(view.recover_draft(snapshot, window, cx));
                assert_eq!(view.input.read(cx).content(), original);
                assert_eq!(view.input.read(cx).selected_range(), selection);
                assert_eq!(view.active_references(cx).len(), 2);
            });
        });
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn editing_keeps_targets_and_drag_selection(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        // Async contribution controls affect the toolbar height before glyph hit testing.
        wait(visual, |cx| {
            let view = view.read(cx);
            view.connected() && view.configured() && view.contributions.read(cx).ready(cx)
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.sidebar = remote;
                view.insert_file_references(vec![("文档 notes.md".into(), false)], window, cx);
            })
        });
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "@文档 notes.md "
        );
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = events.clone();
        let _events = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::File(path) = event {
                    observed.lock().unwrap().push(path.clone());
                }
            })
        });
        click_token(visual, &view, "@文档 notes.md");
        assert_eq!(*events.lock().unwrap(), ["文档 notes.md"]);
        let bounds = view.read_with(visual, |view, cx| {
            let input = view.input.read(cx);
            input.range_to_bounds(&input.tokens()[0].range()).unwrap()
        });
        let from = point(bounds.left() + px(2.), bounds.center().y);
        let to = point(bounds.right() + px(8.), bounds.center().y);
        visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            events.lock().unwrap().len(),
            1,
            "selection must not open links"
        );
        assert!(!view.read_with(visual, |view, cx| {
            view.input.read(cx).selected_range().is_empty()
        }));
        visual.simulate_keystrokes("cmd-a backspace");
        assert!(view.read_with(visual, |view, cx| view.active_references(cx).is_empty()));
        visual.simulate_keystrokes("cmd-z");
        assert_eq!(
            view.read_with(visual, |view, cx| view.active_references(cx)[0]
                .target
                .clone()),
            Target::File("文档 notes.md".into())
        );
        let position = token_position(visual, &view, "@文档 notes.md");
        let state = |visual: &mut VisualTestContext| {
            view.read_with(visual, |view, cx| {
                let input = view.input.read(cx);
                format!(
                    "{:?}",
                    (
                        input.selected_range(),
                        input.tokens(),
                        input.input_bounds(),
                        input.text_bounds(),
                        input.presentation().is_disabled(),
                        view.references.open,
                        view.busy(),
                        view.readonly(),
                    )
                )
            })
        };
        let before = state(visual);
        visual.simulate_mouse_move(position, None, Modifiers::default());
        visual.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
        let down = state(visual);
        visual.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        let after = state(visual);
        assert_eq!(
            events.lock().unwrap().len(),
            2,
            "restored token at {position:?}: before={before}, down={down}, after={after}"
        );
        visual.simulate_keystrokes("cmd-a");
        visual.update(|_, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string("No reference".into()))
        });
        visual.simulate_keystrokes("cmd-v");
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        view.read_with(visual, |view, _| {
            assert!(
                !view
                    .history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part, sailry_protocol::conversation::Part::Reference(_)))
            );
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn formatted_draft_preserves_references_and_submission(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        std::fs::write(
            fixture.directory.path().join("project/notes.md"),
            "Fixture notes",
        )
        .unwrap();
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("# Review ");
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.insert_file_references(vec![("notes.md".into(), false)], window, cx);
            })
        });
        visual.run_until_parked();
        let source = "# Review @notes.md ";
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            source
        );
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("markdown-document").is_none());
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).tokens().len()),
            1
        );
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed = events.clone();
        let _events = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::File(path) = event {
                    observed.lock().unwrap().push(path.clone());
                }
            })
        });
        click_token(visual, &view, "@notes.md");
        assert_eq!(*events.lock().unwrap(), ["notes.md"]);
        let bounds = view.read_with(visual, |view, cx| {
            let input = view.input.read(cx);
            input.range_to_bounds(&input.tokens()[0].range()).unwrap()
        });
        let from = point(bounds.left() + px(2.), bounds.center().y);
        let to = point(bounds.right() + px(8.), bounds.center().y);
        visual.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        visual.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::default());
        visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        visual.run_until_parked();
        assert_eq!(events.lock().unwrap().len(), 1);
        assert!(!view.read_with(visual, |view, cx| {
            view.input.read(cx).selected_range().is_empty()
        }));
        visual.simulate_keystrokes("cmd-a cmd-c backspace");
        assert_eq!(
            visual
                .read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some(source)
        );
        assert!(view.read_with(visual, |view, cx| view.input.read(cx).value().is_empty()));
        visual.simulate_keystrokes("cmd-z");
        visual.run_until_parked();
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            source
        );
        assert_eq!(
            view.read_with(visual, |view, cx| view.active_references(cx)[0]
                .target
                .clone()),
            Target::File("notes.md".into())
        );
        visual.update(|window, cx| {
            let input = view.read(cx).input.clone();
            input.update(cx, |input, cx| {
                input.set_selected_range(source.len()..source.len(), cx)
            });
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "中", None, window, cx);
            });
        });
        visual.run_until_parked();
        tap(visual, "live-chat-send");
        visual.simulate_keystrokes("enter");
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            format!("{source}中")
        );
        visual.update(|window, cx| {
            let input = view.read(cx).input.clone();
            input.update(cx, |input, cx| {
                input.replace_and_mark_text_in_range(None, "", None, window, cx);
            });
        });
        visual.run_until_parked();
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        view.read_with(visual, |view, _| {
            let parts = view.history.snapshot.as_ref().unwrap().page.entries.iter().flat_map(|entry| &entry.parts).collect::<Vec<_>>();
            assert!(parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Text(text) if text == source)));
            assert!(parts.iter().any(|part| matches!(part, sailry_protocol::conversation::Part::Reference(reference) if reference.target == Target::File("notes.md".into()))));
        });
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn disambiguates_targets_and_opens_sessions(cx: &mut TestAppContext) {
    init(cx);
    let fixture = Fixture::with_tools(false, vec![]);
    let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
    wait(visual, |cx| view.read(cx).connected());
    let id = fixture.session.id;
    visual.update(|window, cx| {
        view.update(cx, |view, cx| {
            let first = view.remember_reference(Reference {
                target: Target::Host,
                label: "Same".into(),
            });
            let second = view.remember_reference(Reference {
                target: Target::Session(id),
                label: "Same".into(),
            });
            assert_eq!(first.text(), "@Same");
            assert_eq!(second.text(), "@Same (2)");
            assert_ne!(first.id(), second.id());
            view.input.update(cx, |input, cx| {
                input.set_value("Summarize ", window, cx);
                let end = input.value().len();
                input.set_selected_range(end..end, cx);
                input
                    .replace_with_token(second.clone(), window, cx)
                    .unwrap();
                input.replace(" ", window, cx);
            });
            assert_eq!(view.active_references(cx).len(), 1);
            assert_eq!(
                marker(&view.active_references(cx)[0]),
                second.text().as_str()
            );
        })
    });
    let opened = Arc::new(std::sync::Mutex::new(None));
    let observed = opened.clone();
    let _events = visual.update(|_, cx| {
        cx.subscribe(&view, move |_, event, _| {
            if let Event::Session(id) = event {
                *observed.lock().unwrap() = Some(*id);
            }
        })
    });
    click_token(visual, &view, "@Same (2)");
    assert_eq!(*opened.lock().unwrap(), Some(id));
    visual.update(|window, _| window.remove_window());
    drop(view);
    fixture.close();
}

#[gpui::test]
fn selects_sessions_and_binds_history_tool(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let Output::Session(source) = fixture.execute(Command::CreateSession {
            project: fixture.session.project,
            worktree: Some(fixture.session.worktree),
            config: Some(fixture.session.config.clone()),
        }) else {
            panic!("session expected");
        };
        fixture.execute(Command::RenameSession {
            session: source.id,
            expected_revision: source.revision,
            title: "Design notes 设计".into(),
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Summarize @sessions");
        wait(visual, |cx| {
            view.read(cx).references.list.read(cx).delegate().rows.len() == 1
        });
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| view.read(cx).references.page == Page::Sessions);
        visual.simulate_input("Design");
        wait(visual, |cx| {
            view.read(cx).references.list.read(cx).delegate().rows.len() == 1
        });
        visual.simulate_keystrokes("enter");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Summarize @Design notes 设计 "
        );
        tap(visual, "live-chat-send");
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == Status::Completed)
                })
        });
        view.read_with(visual, |view, _| {
            assert!(
                view.history
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .page
                    .entries
                    .iter()
                    .flat_map(|entry| &entry.parts)
                    .any(|part| matches!(part,
                        sailry_protocol::conversation::Part::Reference(reference)
                            if reference.target == Target::Session(source.id)
                                && reference.label == "Design notes 设计"
                    ))
            );
        });
        let turn = view.read_with(visual, |view, _| {
            view.history.snapshot.as_ref().unwrap().page.runs[0].turn
        });
        let opened = Arc::new(std::sync::Mutex::new(None));
        let observed = opened.clone();
        let _events = visual.update(|_, cx| {
            cx.subscribe(&view, move |_, event, _| {
                if let Event::Session(id) = event {
                    *observed.lock().unwrap() = Some(*id);
                }
            })
        });
        tap(visual, &format!("live-user-text-{turn}"));
        assert_eq!(*opened.lock().unwrap(), Some(source.id));
        let requests = fixture.server.requests.lock().unwrap();
        let request = &requests[0];
        let tool = request["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["function"]["name"] == "read_session")
            .expect("referenced history tool");
        assert_eq!(
            tool["function"]["parameters"]["properties"]["session"]["enum"],
            serde_json::json!([source.id])
        );
        assert!(
            request["messages"]
                .to_string()
                .contains(&source.id.to_string())
        );
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
