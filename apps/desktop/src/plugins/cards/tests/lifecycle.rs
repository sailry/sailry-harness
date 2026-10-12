use super::*;
use crate::conversation::live::tests::{cards as fixtures, fixture, wait};
use serde_json::json;

#[gpui::test]
fn refreshes_without_rewriting_or_replaying(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(
            remote,
            vec![(
                crate::agent_fixture::plugin_tool("card-fixture", "connection"),
                json!({}),
            )],
        );
        fixtures::install(&fixture);
        fixtures::submit(&fixture);
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| {
            view.read(cx)
                .card_instances()
                .first()
                .is_some_and(|card| card.read(cx).active(cx))
        });
        let card = view.read_with(visual, |view, _| view.card_instances()[0].clone());
        let canonical = card.read_with(visual, |card, _| card.summary.clone());
        let requests = fixture.task_requests();
        fixture::tap(visual, "fixture-refresh");
        wait(visual, |cx| {
            card.read(cx)
                .mounted
                .as_ref()
                .is_some_and(|mounted| mounted.host.props()["payload"] == "second")
        });
        fixture::tap(visual, "fixture-refresh");
        wait(visual, |cx| {
            card.read(cx)
                .mounted
                .as_ref()
                .is_some_and(|mounted| mounted.host.props()["payload"] == "first")
        });
        assert_eq!(
            card.read_with(visual, |card, _| card.summary.clone()),
            canonical
        );
        assert_eq!(fixture.task_requests(), requests);
        card.update_in(visual, |card, _, cx| {
            card.suspend();
            cx.notify();
        });
        assert!(card.read_with(visual, |card, _| card.mounted.is_none()
            && card.authorization.url.is_none()));
        fixture.close();
    }
}
