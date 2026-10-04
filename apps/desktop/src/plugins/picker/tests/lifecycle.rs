use super::*;
use gpui_kit::component::input::{Input, InputState};

struct Harness {
    declarations: Vec<(String, bool)>,
    input: Entity<InputState>,
    events: Entity<Events>,
    pickers: Pickers,
}

impl Render for Harness {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .p_4()
            .child(Input::new(&self.input))
            .children(self.declarations.iter().map(|(id, open)| {
                super::super::render(
                    id.clone(),
                    Props {
                        open: *open,
                        title: "Search".into(),
                        items: vec![item("entry-0", ""), item("entry-1", "")],
                        loading: false,
                        empty: "Empty".into(),
                        mode: Mode::List,
                    },
                    self.events.clone(),
                    &self.pickers,
                    window,
                    cx,
                )
            }))
    }
}

struct Probe {
    view: Entity<Harness>,
    input: Entity<InputState>,
    pickers: Pickers,
    events: tokio::sync::mpsc::Receiver<Value>,
}

fn mount(cx: &mut TestAppContext) -> (Probe, &mut VisualTestContext) {
    crate::plugins::tests::init(cx);
    let pickers = Rc::new(RefCell::new(BTreeMap::new()));
    let (sender, events) = tokio::sync::mpsc::channel(32);
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| InputState::new(window, cx).default_value("Draft"));
        let events = cx.new(|_| Events::new(sender, CancellationToken::new()));
        let view = cx.new(|_| Harness {
            declarations: Vec::new(),
            input: input.clone(),
            events,
            pickers: pickers.clone(),
        });
        owner = Some((view.clone(), input));
        Root::new(view, window, cx)
    });
    let (view, input) = owner.unwrap();
    visual.update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
    draw(visual);
    (
        Probe {
            view,
            input,
            pickers,
            events,
        },
        visual,
    )
}

fn draw(visual: &mut VisualTestContext) {
    for _ in 0..2 {
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
    }
    visual.run_until_parked();
}

impl Probe {
    fn show(&self, declarations: &[(&str, bool)], visual: &mut VisualTestContext) {
        self.view.update(visual, |view, cx| {
            view.declarations = declarations
                .iter()
                .map(|(id, open)| ((*id).into(), *open))
                .collect();
            cx.notify();
        });
        draw(visual);
    }

    fn list(&self, id: &str, visual: &mut VisualTestContext) -> WeakEntity<ListState<Rows>> {
        visual.update(|_, cx| {
            let state = self.pickers.borrow()[id].upgrade().unwrap();
            let Control::List(list) = &state.read(cx).control else {
                panic!("list picker expected");
            };
            list.downgrade()
        })
    }

    fn returned(&self, visual: &mut VisualTestContext) {
        assert!(visual.update(|window, cx| self.input.focus_handle(cx).is_focused(window)));
        assert_eq!(
            self.input.read_with(visual, |input, _| input.value()),
            "Draft"
        );
        assert!(self.pickers.borrow().is_empty());
    }
}

#[gpui::test]
fn releases_closed_placeholders_and_unmounted_views(cx: &mut TestAppContext) {
    let (mut probe, visual) = mount(cx);
    for (open, closed) in [
        ("picker", "picker"),
        ("git-picker-7", "git-picker-0"),
        ("worktree-token-7", "worktree-picker"),
    ] {
        probe.show(&[(open, true)], visual);
        let list = probe.list(open, visual);
        assert!(
            visual.update(|window, cx| list.upgrade().unwrap().focus_handle(cx).is_focused(window))
        );
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        let close = json!({"picker":open,"kind":"close"});
        assert_eq!(probe.events.try_recv().unwrap(), close);
        // Kit's List propagates Cancel to its Dialog, which also reports close.
        while let Ok(event) = probe.events.try_recv() {
            assert_eq!(event, close);
        }
        probe.show(&[(closed, false)], visual);
        assert!(list.upgrade().is_none());
        probe.returned(visual);
    }
    probe.show(&[("unmounted", true)], visual);
    let list = probe.list("unmounted", visual);
    probe.show(&[], visual);
    assert!(list.upgrade().is_none());
    probe.returned(visual);
}

#[gpui::test]
fn replaces_ids_without_losing_return_focus(cx: &mut TestAppContext) {
    let (probe, visual) = mount(cx);
    probe.show(&[("git-picker-1", true)], visual);
    let previous = probe.list("git-picker-1", visual);
    visual.simulate_input("entry-1");
    visual
        .executor()
        .advance_clock(std::time::Duration::from_millis(100));
    draw(visual);
    assert_eq!(
        previous
            .upgrade()
            .unwrap()
            .read_with(visual, |list, _| list.delegate().query.clone()),
        "entry-1"
    );
    probe.show(&[("git-picker-2", true)], visual);
    assert!(previous.upgrade().is_none());
    assert_eq!(probe.pickers.borrow().len(), 1);
    let current = probe.list("git-picker-2", visual);
    assert!(visual.update(|window, cx| {
        current
            .upgrade()
            .unwrap()
            .focus_handle(cx)
            .is_focused(window)
    }));
    assert_eq!(
        current
            .upgrade()
            .unwrap()
            .read_with(visual, |list, _| list.delegate().query.clone()),
        ""
    );
    probe.show(&[("git-picker-0", false)], visual);
    assert!(current.upgrade().is_none());
    probe.returned(visual);
}

#[gpui::test]
fn isolates_declarations_and_preserves_new_focus(cx: &mut TestAppContext) {
    let (probe, visual) = mount(cx);
    probe.show(&[("closed", false), ("first", true)], visual);
    let first = probe.list("first", visual);
    let first_id = first.entity_id();
    probe.show(&[("first", true), ("closed", false)], visual);
    assert_eq!(probe.list("first", visual).entity_id(), first_id);
    assert!(
        visual.update(|window, cx| first.upgrade().unwrap().focus_handle(cx).is_focused(window))
    );
    probe.show(&[("first", true), ("second", true)], visual);
    let second = probe.list("second", visual);
    assert!(visual.update(|window, cx| {
        second
            .upgrade()
            .unwrap()
            .focus_handle(cx)
            .is_focused(window)
    }));
    probe.show(&[("first", true), ("second", false)], visual);
    assert!(second.upgrade().is_none());
    assert!(
        visual.update(|window, cx| first.upgrade().unwrap().focus_handle(cx).is_focused(window))
    );
    probe.show(&[("first", true), ("second", true)], visual);
    let second = probe.list("second", visual);
    probe.show(&[("first", false), ("second", true)], visual);
    assert!(first.upgrade().is_none());
    assert!(visual.update(|window, cx| {
        second
            .upgrade()
            .unwrap()
            .focus_handle(cx)
            .is_focused(window)
    }));
    probe.show(&[("first", false), ("second", false)], visual);
    assert!(second.upgrade().is_none());
    probe.returned(visual);
}
