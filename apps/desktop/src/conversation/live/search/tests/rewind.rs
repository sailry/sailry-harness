use super::*;

#[gpui::test]
fn refreshes_external_rewind(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, Vec::new());
        let first = submit(&fixture, "History retained 中文 🙂".into());
        let last = submit(&fixture, "History removed 中文 🙂".into());
        let (view, visual) = open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Keep this draft 🙂");
        tap(visual, "live-search");
        visual.simulate_input("History");
        searched(&view, visual, 2);
        fixture.execute(Command::RewindConversation {
            session: fixture.session.id,
            through: Some(first),
            expected_head: last,
            expected_revision: 1,
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .revision
                == 2
        });
        searched(&view, visual, 1);
        view.read_with(visual, |view, cx| {
            let results = view.search.read(cx).list.read(cx).delegate();
            assert_eq!(results.revision, Some(2));
            assert_eq!(results.matches[0].turn, first);
            assert_eq!(results.query, "History");
            assert!(view.reveal.is_none());
            assert_eq!(view.rows, [first]);
            assert_eq!(view.input.read(cx).value(), "Keep this draft 🙂");
        });
        visual.simulate_keystrokes("enter");
        wait(visual, |cx| !view.read(cx).search.read(cx).open);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        fixture.close();
    }
}
