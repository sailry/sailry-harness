use super::*;
use sailry_protocol::{
    ErrorCode, Fault,
    conversation::Part,
    process::{Completion, Outcome},
};

#[gpui_kit::test]
fn reserves_danger_for_execution_errors(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("commands", "run_command"),
                json!({"command":"printf ready"}),
            )],
        );
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        }) else {
            panic!("session expected")
        };
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session.clone());
        fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Inspect tool outcomes".into(),
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .first()
                        .is_some_and(|run| run.status == Status::Completed)
                })
        });
        let history = view.read_with(visual, |view, _| view.history.clone());
        let cases = [
            (
                "read_file",
                json!({"error":Fault::new(ErrorCode::NotFound, "File diagnostic"), "isError":true,
                    "sailry_result":{"version":1,"diagnostics":[],"expected":"file_not_found"}}),
                false,
                None,
            ),
            (
                "list_directory",
                json!({"error":Fault::new(ErrorCode::NotFound, "Directory diagnostic"), "isError":true,
                    "sailry_result":{"version":1,"diagnostics":[],"expected":"directory_not_found"}}),
                false,
                None,
            ),
            (
                "list_directory",
                json!({"error":Fault::new(ErrorCode::PermissionDenied, "Denied diagnostic"), "isError":true,
                    "sailry_result":{"version":1,"diagnostics":[],"expected":"directory_not_found"}}),
                true,
                Some("tool_failed"),
            ),
            (
                "read_file",
                json!({"error":Fault::new(ErrorCode::NotFound, "worktree filesystem operation failed: No such file or directory (os error 2)")}),
                true,
                Some("tool_failed"),
            ),
            (
                "external_tool",
                json!({"error":Fault::new(ErrorCode::PermissionDenied, "Access denied")}),
                true,
                Some("tool_failed"),
            ),
            (
                "external_tool",
                json!({"error":Fault::new(ErrorCode::Cancelled, "Cancelled")}),
                false,
                Some("turn_cancelled"),
            ),
            (
                "external_tool",
                json!({"error":Fault::new(ErrorCode::OutcomeUnknown, "Outcome unknown")}),
                false,
                Some("tool_interrupted"),
            ),
            (
                "computer_input",
                json!({"sailry_result":{"version":1,"diagnostics":[{"text":"Unconfirmed condition","error":false}]}}),
                false,
                None,
            ),
            (
                "computer_input",
                json!({"sailry_result":{"version":1,"diagnostics":[{"text":"Unmet condition","error":false}],"status":{"label":tr("tool_condition_unmet")}}}),
                false,
                None,
            ),
            (
                "computer_input",
                json!({"requires_verification":true}),
                false,
                None,
            ),
            (
                "external_step",
                json!({"response":{"error":"Opaque response"},"sailry_result":{"version":1,"diagnostics":[{"text":"Unconfirmed condition","error":false}]}}),
                false,
                None,
            ),
            (
                "computer_input",
                json!({"sailry_result":{"version":1,"diagnostics":[{"text":"Capture unavailable","error":true}]}}),
                true,
                Some("tool_failed"),
            ),
            (
                "computer_input",
                json!({"sailry_result":{"version":1,"diagnostics":[{"text":"Worker disconnected","error":true}]}}),
                true,
                Some("tool_failed"),
            ),
            (
                "external_step",
                json!({"isError":true,"sailry_result":{"version":1,"diagnostics":[{"text":"Unmet condition","error":false}],"status":{"label":tr("tool_condition_unmet")}}}),
                true,
                Some("tool_failed"),
            ),
            (
                "external_tool",
                json!({"error":Fault::new(ErrorCode::Unavailable, "Worker unavailable")}),
                true,
                Some("tool_failed"),
            ),
            (
                "external_tool",
                {
                    let mut result = serde_json::to_value(Output::CommandResult(Completion {
                        outcome: Outcome::TimedOut,
                        stdout: Default::default(),
                        stderr: Default::default(),
                        elapsed_ms: 100,
                    }))
                    .unwrap();
                    result["isError"] = true.into();
                    result
                },
                true,
                Some("tool_failed"),
            ),
            (
                "empty_structured",
                json!({"stdout":"", "stderr":"", "isError":true}),
                true,
                Some("tool_failed"),
            ),
            (
                "empty_cancelled",
                json!({"error":Fault::new(ErrorCode::Cancelled, "")}),
                false,
                Some("turn_cancelled"),
            ),
        ];
        for (name, result, danger, label) in cases {
            let diagnostic = output::failure(name, Some(&result));
            let mut sample = history.clone();
            let call = &mut Arc::make_mut(&mut sample.calls)[0];
            call.name = name.into();
            call.content = (name == "empty_structured").then(|| sailry_protocol::tool::Content {
                version: 1,
                blocks: vec![sailry_protocol::tool::Block::Text(
                    sailry_protocol::tool::Text {
                        path: "/stdout".into(),
                        notices: Vec::new(),
                    },
                )],
            });
            call.presentation = sailry_protocol::tool::Presentation::Summary;
            let reference = call.response.as_ref().unwrap();
            let page = Arc::make_mut(&mut Arc::make_mut(sample.snapshot.as_mut().unwrap()).page);
            let source = page
                .entries
                .iter_mut()
                .find(|entry| entry.id == call.source.entry)
                .unwrap();
            let Part::ToolCall {
                name: tool_name,
                display,
                presentation,
                ..
            } = &mut source.parts[call.source.index]
            else {
                panic!("tool call expected")
            };
            *tool_name = name.into();
            *display = None;
            *presentation = call.presentation;
            let entry = page
                .entries
                .iter_mut()
                .find(|entry| entry.id == reference.entry)
                .unwrap();
            let Part::ToolResult {
                name: tool_name,
                result: body,
                ..
            } = &mut entry.parts[reference.index]
            else {
                panic!("tool result expected")
            };
            *tool_name = name.into();
            *body = result;
            let row = summary(call, page);
            assert_eq!(row.detail.failed, danger, "remote={remote}, name={name}");
            assert_eq!(row.detail.state, label.map(tr));
            assert_eq!(
                row.detail.state_icon.is_some(),
                matches!(label, Some("turn_cancelled" | "tool_interrupted"))
            );
            if diagnostic
                .as_ref()
                .is_none_or(|message| message.trim().is_empty())
                && output::is_error(name, call.result(page))
            {
                assert!(!has_content(call, page));
            }

            let group = group_summary(&[call], page);
            assert_eq!(group.detail.failed, danger);
            assert_eq!(group.detail.state, label.map(tr));
            assert_eq!(output::failure(name, call.result(page)), diagnostic);
            if output::fault(call.result(page)).is_some() {
                let preview =
                    output::tool_preview(name, call.presentation, call.result(page).unwrap());
                let diagnostic = diagnostic.as_ref().unwrap();
                if name == "empty_cancelled" {
                    assert!(diagnostic.is_empty());
                    assert!(preview.text.is_empty());
                    assert_eq!(preview.notices, vec!["turn_cancelled"]);
                    assert_eq!(preview.copy_text(), tr("turn_cancelled").to_string());
                } else {
                    assert_eq!(&preview.text, diagnostic);
                    assert_eq!(&preview.copy_text(), diagnostic);
                }
            }
            let has_result = has_content(call, page);
            if name == "empty_cancelled" {
                assert!(!has_result, "an empty cancellation has no disclosure");
            }
            let selector = format!("live-tool-result-{}-{}", call.turn, call.source.key());
            let key = (call.turn, format!("tools-{}", call.source.key()));
            view.update(visual, |view, cx| {
                view.update_history(sample, cx);
                view.expanded.insert((key.0, "work".into()), true);
                view.expanded.insert(key, true);
            });
            redraw(visual);
            assert_eq!(
                visual
                    .debug_bounds(Box::leak(selector.into_boxed_str()))
                    .is_some(),
                has_result,
                "remote={remote}, name={name}"
            );
        }
        for (package, tool, manifest, target, arguments) in [
            (
                "reminders",
                "reminders",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../plugins/reminders/plugin.json"
                )),
                "Review reminder 中文 🙂",
                json!({"action":"create","title":"Review reminder 中文 🙂","message":"Hidden reminder payload"}),
            ),
            (
                "scheduled-tasks",
                "scheduled_tasks",
                include_str!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../plugins/scheduled-tasks/plugin.json"
                )),
                "Review task 中文 🙂",
                json!({"action":"create","name":"Review task 中文 🙂","prompt":"Hidden task payload",
                    "timing":{"kind":"once","data":{"at_ms":2_000_000_000_000_i64}}}),
            ),
        ] {
            let manifest: serde_json::Value = serde_json::from_str(manifest).unwrap();
            let declaration = &manifest["extensions"]["dev.sailry.platform"]["tools"][0];
            let presentation: sailry_protocol::tool::Presentation =
                serde_json::from_value(declaration["presentation"].clone()).unwrap();
            assert_eq!(presentation, sailry_protocol::tool::Presentation::Summary);
            let captured: sailry_protocol::tool::Display =
                serde_json::from_value(declaration["display"].clone()).unwrap();
            let label = captured.label(&rust_i18n::locale()).to_owned();
            let field = captured.input.as_ref().unwrap().summary.as_ref().unwrap();
            let input = sailry_client::conversation::tools::display::Input {
                summary: Some(sailry_client::conversation::tools::display::Text {
                    text: field.read(&arguments).unwrap().to_owned(),
                    code: field.code,
                }),
                ..Default::default()
            };
            let mut sample = history.clone();
            let call = &mut Arc::make_mut(&mut sample.calls)[0];
            call.name = crate::agent_fixture::plugin_tool(package, tool);
            call.content = None;
            call.presentation = presentation;
            // The synthetic call uses this package's captured input recipe,
            // rather than the original command call's resolved input.
            call.resolved.as_mut().unwrap().input = input;
            let reference = call.response.as_ref().unwrap();
            let page = Arc::make_mut(&mut Arc::make_mut(sample.snapshot.as_mut().unwrap()).page);
            let source = page
                .entries
                .iter_mut()
                .find(|entry| entry.id == call.source.entry)
                .unwrap();
            let Part::ToolCall {
                name,
                arguments: input,
                display,
                presentation,
                ..
            } = &mut source.parts[call.source.index]
            else {
                panic!("tool call expected")
            };
            *name = call.name.clone();
            *input = arguments;
            *display = Some(Box::new(captured));
            *presentation = call.presentation;
            let response = page
                .entries
                .iter_mut()
                .find(|entry| entry.id == reference.entry)
                .unwrap();
            let Part::ToolResult { name, result, .. } = &mut response.parts[reference.index] else {
                panic!("tool result expected")
            };
            *name = call.name.clone();
            *result = json!({"id":"stored","payload":"Hidden result payload"});
            let summary = summary(call, page);
            assert_eq!(summary.label.as_ref(), label);
            assert_eq!(summary.detail.text.as_ref(), target);
            assert!(summary.detail.hide_caret);
            assert!(!has_content(call, page));
            let key = (call.turn, format!("tools-{}", call.source.key()));
            let row = format!("live-{}-{}", key.0, key.1);
            let output = format!("live-tool-result-{}-{}", call.turn, call.source.key());
            view.update(visual, |view, cx| {
                view.update_history(sample, cx);
                view.expanded.insert((key.0, "work".into()), true);
                view.expanded.insert(key, true);
            });
            redraw(visual);
            let bounds = visual
                .debug_bounds(Box::leak(row.clone().into_boxed_str()))
                .expect("summary row");
            let inline = visual
                .debug_bounds(Box::leak(format!("{row}-summary").into_boxed_str()))
                .expect("inline label and target");
            assert_eq!(bounds.size.height, px(24.));
            assert!(inline.size.height <= bounds.size.height);
            assert!(
                visual
                    .debug_bounds(Box::leak(output.into_boxed_str()))
                    .is_none()
            );
        }
        for state in [State::Cancelled, State::NotExecuted, State::Interrupted] {
            let mut call = history.calls[0].clone();
            call.response = None;
            call.state = state;
            let detail = summary(&call, &history.snapshot.as_ref().unwrap().page).detail;
            assert!(!detail.failed);
            assert!(detail.state.is_some());
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
