use super::{Link, State, View};
use gpui_kit as gpui;
use gpui_kit::component::Root;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    App, AppContext as _, Bounds, Context, Entity, IntoElement, Modifiers, Pixels, Render,
    SharedString, TestAppContext, VisualTestContext, Window, px,
};
use std::{cell::RefCell, rc::Rc};

struct Prompt {
    state: Entity<State>,
    opened: Rc<RefCell<Vec<SharedString>>>,
}

impl Render for Prompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let opened = self.opened.clone();
        let start = self.state.read(cx).text.find("@notes.md").unwrap();
        View::new(&self.state, "fixture", px(14.)).links(
            vec![Link {
                range: start..start + "@notes.md".len(),
                id: "reference".into(),
            }],
            move |link, _, _| opened.borrow_mut().push(link.id.clone()),
        )
    }
}

fn settle(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.render_frame(cx));
    visual.run_until_parked();
}

impl State {
    pub(crate) fn reference_bounds(&self, text: &str, cx: &App) -> Option<Bounds<Pixels>> {
        let input = self.references.as_ref()?.read(cx);
        let span = input
            .tokens()
            .iter()
            .find(|span| span.token().text() == text)?;
        input.range_to_bounds(&span.range())
    }
}

#[gpui::test]
fn references_preserve_literal_source(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let source = "**Literal** text @notes.md\nNext line";
    let range = source.find("@notes.md").unwrap();
    let opened = Rc::new(RefCell::new(Vec::new()));
    let mut state = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| State::new(source, cx));
        state = Some(input.clone());
        let prompt = cx.new(|_| Prompt {
            state: input,
            opened: opened.clone(),
        });
        Root::new(prompt, window, cx)
    });
    settle(visual);
    let state = state.unwrap();
    let input = state.read_with(visual, |state, _| state.references.clone().unwrap());
    visual.update(|window, cx| {
        input.update(cx, |input, cx| input.focus(window, cx));
    });
    visual.simulate_input("ignored");
    visual.simulate_keystrokes("cmd-a cmd-c");
    assert_eq!(
        visual.read_from_clipboard().unwrap().text().as_deref(),
        Some(source)
    );
    input.read_with(visual, |input, _| {
        assert_eq!(input.value(), source);
        let [span] = input.tokens() else {
            panic!("one captured reference expected")
        };
        assert_eq!(span.range(), range..range + "@notes.md".len());
    });
    let position = visual.update(|_, cx| {
        input
            .read(cx)
            .range_to_bounds(&(range..range + "@notes.md".len()))
            .unwrap()
            .center()
    });
    visual.simulate_mouse_move(position, None, Modifiers::default());
    settle(visual);
    crate::conversation::live::tests::fixture::press(visual, position);
    settle(visual);
    assert_eq!(*opened.borrow(), [SharedString::from("reference")]);
    assert_eq!(input.read_with(visual, |input, _| input.value()), source);
}
