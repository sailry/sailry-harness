use super::*;

#[gpui_kit::test]
fn keeps_diagnostics_expandable(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("files", "list_directory"),
                    json!({"path":"missing-directory"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("files", "read_file"),
                    json!({"path":"missing-file.txt"}),
                ),
            ],
        );
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        fixture.start();
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
        let page = &history.snapshot.as_ref().unwrap().page;
        assert_eq!(history.calls.len(), 2);
        let turn = history.calls[0].turn;
        fixture::tap(visual, &format!("live-turn-work-{turn}"));
        // Each read is a separate activity; the surrounding sequence remains interactive.
        fixture::tap(
            visual,
            &format!(
                "live-{turn}-tool-sequence-{}",
                history.calls[0].source.key()
            ),
        );
        for (call, key) in history
            .calls
            .iter()
            .zip(["tool_directory_missing", "tool_file_missing"])
        {
            let row = summary(call, page);
            assert!(!row.detail.failed);
            assert_eq!(row.detail.state, Some(tr(key)));
            assert!(!row.detail.text.contains("filesystem operation failed"));
            let diagnostic = output::failure(&call.name, call.result(page)).unwrap();
            assert!(diagnostic.contains("worktree filesystem operation failed"));
            let row = format!(
                "live-{turn}-{}-{}",
                if projection::grouped(call, page) {
                    "tool-group"
                } else {
                    "tools"
                },
                call.source.key()
            );
            fixture::tap(visual, &row);
            let error = format!("live-tool-error-{turn}-{}", call.source.key());
            assert!(
                visual
                    .debug_bounds(Box::leak(error.into_boxed_str()))
                    .is_some()
            );
            fixture::tap(
                visual,
                &format!("live-tool-copy-{turn}-{}", call.source.key()),
            );
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
                diagnostic
            );
        }
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
