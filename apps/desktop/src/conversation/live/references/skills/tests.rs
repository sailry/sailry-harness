use super::*;
use crate::conversation::live::tests::{
    fixture::{self, Fixture, init, tap},
    wait,
};
use core::prelude::v1::test;
use serde_json::json;

fn native_reference(view: &View, cx: &App) {
    let references = view.active_references(cx);
    let [reference] = references.as_slice() else {
        panic!("one captured reference expected");
    };
    let input = view.input.read(cx);
    assert!(input.links().is_empty());
    let [span] = input.tokens() else {
        panic!("one native input token expected");
    };
    assert_eq!(
        span.token().id(),
        super::super::inline::token(reference).id()
    );
    assert!(super::super::inline::draft_icon(reference).is_none());
}

fn install(fixture: &Fixture, name: &str, skill: &str, revision: u64) -> plugin::Info {
    let root = fixture.directory.path().join("project").join(name);
    std::fs::create_dir_all(root.join(format!("skills/{skill}"))).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        json!({
            "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name":name
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(root.join(format!("skills/{skill}/SKILL.md")), format!(
        "---\nname: {skill}\ndescription: Inspect project changes\n---\n\nOriginal {name} instructions\n"
    )).unwrap();
    let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
        worktree: fixture.binding.worktree.unwrap(),
        path: name.into(),
        name: name.into(),
        expected_revision: revision,
    }) else {
        panic!("plugin expected");
    };
    info
}

fn shown(visual: &mut VisualTestContext, selector: &'static str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        if visual.debug_bounds(selector).is_some() {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "missing {selector}");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[gpui::test]
fn plugin_links_reuse_details(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let package = install(&fixture, "reports", "analysis", 0);
        let root = fixture.directory.path().join("project/reports");
        std::fs::create_dir_all(root.join("dev.sailry.platform")).unwrap();
        std::fs::write(
            root.join("plugin.json"),
            json!({
                "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name":"reports", "version":"1.0.0",
                "extensions":{"dev.sailry.platform":{
                    "api_version":"v1", "actions":[], "settings_schema":"dev.sailry.platform/settings.json"
                }}
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            root.join("dev.sailry.platform/settings.json"),
            json!({
                "$schema":plugin::settings::SCHEMA,
                "type":"object", "additionalProperties":false,
                "properties":{"label":{"type":"string", "title":"Label", "default":"Fixture value"}}
            })
            .to_string(),
        )
        .unwrap();
        let Output::Plugin(info) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.binding.worktree.unwrap(),
            path: "reports".into(),
            name: "reports".into(),
            expected_revision: package.summary.revision,
        }) else {
            panic!("plugin expected");
        };
        assert!(
            info.settings.is_some(),
            "invalid settings fixture: {:?}",
            info.issues
        );
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("@reports");
        wait(visual, |cx| {
            !view.read(cx).references.loading
                && view
                    .read(cx)
                    .references
                    .list
                    .read(cx)
                    .delegate()
                    .rows
                    .iter()
                    .any(
                        |item| matches!(item, Item::Plugin(info) if info.summary.name == "reports"),
                    )
        });
        visual.simulate_keystrokes("enter");
        view.read_with(visual, |view, cx| {
            assert_eq!(view.input.read(cx).value(), "@reports ");
            assert_eq!(
                view.active_references(cx)[0].target,
                Target::Plugin("reports".into())
            );
            native_reference(view, cx);
        });
        super::super::tests::inline::click_token(visual, &view, "@reports");
        shown(visual, "plugin-skill-analysis");
        shown(visual, "plugin-details-configure");
        assert!(visual.debug_bounds("details-content").is_none());
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.simulate_keystrokes("escape");
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
        let turn = view.read_with(visual, |view, _| {
            let page = &view.history.snapshot.as_ref().unwrap().page;
            let user = page
                .entries
                .iter()
                .find(|entry| entry.author == "user")
                .unwrap();
            assert!(user.parts.iter().any(|part| matches!(part,
                sailry_protocol::conversation::Part::Reference(reference)
                    if reference.target == Target::Plugin("reports".into())
            )));
            user.turn
        });
        tap(visual, &format!("live-user-text-{turn}"));
        shown(visual, "plugin-skill-analysis");
        shown(visual, "plugin-details-configure");
        assert!(visual.debug_bounds("details-content").is_none());
        tap(visual, "plugin-details-configure");
        shown(visual, "plugin-setting-label");
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.simulate_keystrokes("escape escape");
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn selects_and_loads_versions(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let Output::Plugins(packages) = fixture.execute(Command::ListPlugins) else {
            panic!("plugin inventory expected");
        };
        for package in packages.into_iter().filter(|package| package.enabled) {
            fixture.execute(Command::SetPluginEnabled {
                name: package.name,
                expected_revision: package.revision,
                enabled: false,
            });
        }
        let package = install(&fixture, "example", "analysis", 0);
        install(&fixture, "other", "analysis", 0);
        let disabled = install(&fixture, "disabled", "analysis", 0);
        fixture.execute(Command::SetPluginEnabled {
            name: disabled.summary.name,
            expected_revision: disabled.summary.revision,
            enabled: false,
        });
        install(
            &fixture,
            "example",
            "new-analysis",
            package.summary.revision,
        );
        let (draft, visual) = fixture::open_session(cx, fixture.binding.clone(), None);
        wait(visual, |cx| draft.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("/");
        wait(visual, |cx| !draft.read(cx).references.loading);
        draft.read_with(visual, |view, cx| {
            assert!(!view.references.error);
            assert_eq!(view.skill_packages().len(), 2);
            let rows = &view.references.list.read(cx).delegate().rows;
            assert!(rows.iter().any(|item| item.label() == "New Analysis"));
            assert!(rows.iter().any(|item| item.label() == "Analysis"));
            assert!(!rows.iter().any(|item| item.label().contains("disabled")));
            assert!(!rows.iter().any(|item| matches!(item, Item::Plugin(_))));
        });
        visual.simulate_input("other:analysis");
        visual.simulate_keystrokes("enter");
        assert_eq!(
            draft.read_with(visual, |view, cx| view.input.read(cx).value()),
            "@Analysis "
        );
        draft.read_with(visual, |view, cx| {
            native_reference(view, cx);
        });
        assert!(draft.read_with(visual, |view, _| view.session.is_none()));
        visual.update(|window, _| window.remove_window());
        drop(draft);

        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Please use /new-analysis");
        wait(visual, |cx| !view.read(cx).references.loading);
        view.read_with(visual, |view, cx| {
            let rows = &view.references.list.read(cx).delegate().rows;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].label(), "New Analysis");
        });
        tap(visual, "live-reference-row-0");
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Please use @New Analysis "
        );
        super::super::tests::inline::click_token(visual, &view, "@New Analysis");
        shown(visual, "details-content");
        visual.update(|window, cx| window.close_dialog(cx));
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.input.update(cx, |input, cx| {
                    let end = input.value().len();
                    input.set_selected_range(end..end, cx);
                    input.focus(window, cx);
                });
            })
        });
        visual.simulate_input("Inspect the project");
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
            assert!(view.history.snapshot.as_ref().unwrap().page.entries.iter().flat_map(|entry| &entry.parts).any(|part| {
                matches!(part, sailry_protocol::conversation::Part::Reference(Reference { target: Target::Skill { package, name }, .. }) if package == "example" && name == "new-analysis")
            }));
        });
        assert!(
            fixture.server.requests.lock().unwrap()[0]["messages"]
                .to_string()
                .contains("Original example instructions")
        );
        assert!(
            fixture.server.requests.lock().unwrap()[0]
                .to_string()
                .contains("Please use @New Analysis Inspect the project")
        );
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn plugin_mentions_select_capabilities(cx: &mut TestAppContext) {
    use gpui_kit::component::list::ListDelegate;
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let package = install(&fixture, "reports", "analysis", 0);
        fixture.execute(Command::SetPluginEnabled {
            name: "computer".into(),
            expected_revision: 1,
            enabled: true,
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("@");
        wait(visual, |cx| !view.read(cx).references.loading);
        view.read_with(visual, |view, cx| {
            let list = view.references.list.read(cx).delegate();
            assert_eq!(list.sections_count(cx), 2);
            assert!(
                list.rows
                    .iter()
                    .any(|item| matches!(item, Item::Page(Page::Files(_))))
            );
            assert!(
                list.rows
                    .iter()
                    .any(|item| matches!(item, Item::Page(Page::Ssh)))
            );
            assert!(
                list.rows
                    .iter()
                    .any(|item| matches!(item, Item::Page(Page::Databases)))
            );
            let info = list
                .rows
                .iter()
                .find_map(|item| match item {
                    Item::Plugin(info) if info.summary.name == "computer" => Some(info),
                    _ => None,
                })
                .expect("native capability suggestion");
            let extension = info.extension.as_ref().unwrap();
            assert!(extension.tools.is_empty());
            assert!(extension.instructions.is_none());
            assert!(
                !list
                    .rows
                    .iter()
                    .any(|item| matches!(item, Item::Command(commands::Choice::Skill(..))))
            );
            for index in 0..list.rows.len() {
                let path = list.index(index);
                assert_eq!(list.flat_index(path), Some(index));
            }
        });
        visual.simulate_input("computer");
        visual.simulate_keystrokes("enter");
        view.read_with(visual, |view, cx| {
            assert_eq!(
                view.active_references(cx)[0].target,
                Target::Plugin("computer".into())
            );
            native_reference(view, cx);
        });
        visual.simulate_input(" Describe the available capability without controlling any app");
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
        let request = fixture.server.requests.lock().unwrap()[0].clone();
        let tools = request["tools"].as_array().unwrap();
        for name in ["list_windows", "get_window_state", "click"] {
            assert!(
                tools.iter().any(|tool| tool["function"]["name"] == name),
                "missing native tool {name}"
            );
        }
        let request = request.to_string();
        assert!(request.contains("explicitly selected plugin computer"));
        assert!(request.contains("Available tools:"));
        assert!(!request.contains("computer_desktop"));
        assert!(!request.contains("Computer operation workflow"));
        // A disabled plugin cannot be re-enabled or invoked by a forged mention.
        fixture.execute(Command::SetPluginEnabled {
            name: package.summary.name.clone(),
            expected_revision: package.summary.revision,
            enabled: false,
        });
        let client = &fixture.binding.client;
        let result = fixture
            .binding
            .runtime
            .block_on(client.execute(client.prepare(Command::SubmitTurn {
                session: fixture.session.id,
                expected_revision: fixture.session.revision,
                message: sailry_protocol::conversation::Input {
                    text: "@Reports".into(),
                    attachments: vec![],
                    references: vec![Reference {
                        label: "Reports".into(),
                        target: Target::Plugin("reports".into()),
                    }],
                },
            })));
        assert_eq!(
            result.unwrap_err().message,
            "referenced plugin is disabled or unavailable"
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn queued_mentions_keep_the_admitted_skill(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let package = install(&fixture, "reports", "analysis", 0);
        let message = sailry_protocol::conversation::Input {
            text: "Use @Budget Analysis for this task".into(),
            attachments: vec![],
            references: vec![Reference {
                label: "Budget Analysis".into(),
                target: Target::Skill {
                    package: "reports".into(),
                    name: "analysis".into(),
                },
            }],
        };
        let Output::QueuedTurn(turn) = fixture.execute(Command::QueueTurn {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            message: message.clone(),
        }) else {
            panic!("queued turn expected")
        };
        let root = fixture.directory.path().join("project/reports");
        std::fs::write(
            root.join("skills/analysis/SKILL.md"),
            "---\nname: analysis\ndescription: Revised analysis\n---\nReplacement workflow\n",
        )
        .unwrap();
        let Output::Plugin(updated) = fixture.execute(Command::InstallPlugin {
            worktree: fixture.binding.worktree.unwrap(),
            path: "reports".into(),
            name: "reports".into(),
            expected_revision: package.summary.revision,
        }) else {
            panic!("plugin expected")
        };
        fixture.execute(Command::SetPluginEnabled {
            name: "reports".into(),
            expected_revision: updated.summary.revision,
            enabled: false,
        });
        fixture.execute(Command::EditQueuedTurn {
            turn: turn.id,
            expected_revision: 1,
            message,
        });
        fixture.execute(Command::StartQueuedTurn { turn: turn.id });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
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
                        .any(|run| run.turn == turn.id && run.status == Status::Completed)
                })
        });
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let prompt = requests[0]["messages"].to_string();
        assert!(prompt.contains("Original reports instructions"));
        assert!(!prompt.contains("Replacement workflow"));
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
