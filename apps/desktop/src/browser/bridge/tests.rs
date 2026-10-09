use super::*;
use crate::conversation::live::tests::{fixture, wait};
use core::prelude::v1::test;

#[gpui_kit::test]
fn binds_context_to_the_owning_composer(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = fixture::Fixture::with_tools(remote, vec![]);
        let (source, visual) = fixture::open(cx, fixture.binding.clone(), fixture.session.clone());
        wait(visual, |cx| source.read(cx).connected());
        visual.update(|window, cx| {
            let browser = cx.new(|cx| Browser::new(true, window, cx));
            let other = cx.new(|cx| Browser::new(true, window, cx));
            let node = fixture.binding.client.target();
            let key = Key::Session(node, fixture.session.id);
            let mut state = State::default();
            state
                .views
                .insert(Owner::Session(node, fixture.session.id), browser.clone());
            assert!(state.owns_composer(key, &source, &browser, cx));
            assert!(!state.owns_composer(key, &source, &other, cx));
            assert!(!state.owns_composer(
                Key::Session(NodeId([0xff; 32]), fixture.session.id),
                &source,
                &browser,
                cx
            ));
            assert!(!state.owns_composer(
                Key::Session(node, SessionId::new()),
                &source,
                &browser,
                cx
            ));
            assert!(!state.owns_composer(Key::Draft, &source, &browser, cx));
            state.views.clear();
            assert!(!state.owns_composer(key, &source, &browser, cx));
        });
        visual.update(|window, _| window.remove_window());
        drop(source);
        fixture.close();
    }
}
