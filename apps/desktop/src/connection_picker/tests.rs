use super::*;
use core::prelude::v1::test;
use std::cell::RefCell;

struct Frame;
impl Render for Frame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

#[gpui::test]
fn dispatches_selected_resource(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Frame), window, cx));
    let chosen = Rc::new(RefCell::new(None));
    let target = sailry_protocol::DatabaseId::new();
    visual.update(|window, cx| {
        let chosen = chosen.clone();
        open(
            Kind::Database,
            vec![
                Entry {
                    resource: Resource::Database(sailry_protocol::DatabaseId::new()),
                    name: "Local".into(),
                    detail: "local.test".into(),
                },
                Entry {
                    resource: Resource::Database(target),
                    name: "Staging".into(),
                    detail: "remote.test".into(),
                },
            ],
            window,
            cx,
            move |choice, _, _| *chosen.borrow_mut() = Some(choice),
        );
    });
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(visual);
    let full = visual.debug_bounds("connection-picker").unwrap();
    assert_eq!(
        visual.debug_bounds("command-search").unwrap().size.height,
        px(45.)
    );
    let connection = visual.debug_bounds("connection-choice-0").unwrap();
    let add = visual.debug_bounds("connection-add").unwrap();
    assert!(connection.size.height > add.size.height);
    assert!(connection.size.height <= px(36.));
    visual.simulate_input("remote.test");
    draw(visual);
    assert!(
        visual
            .debug_bounds("connection-picker")
            .unwrap()
            .size
            .height
            < full.size.height
    );
    assert!(visual.debug_bounds("connection-choice-0").is_none());
    assert!(visual.debug_bounds("connection-choice-1").is_some());
    visual.simulate_keystrokes("enter");
    draw(visual);
    assert!(visual.debug_bounds("connection-action-open").is_some());
    assert!(visual.debug_bounds("connection-action-transfer").is_none());
    let back = visual.debug_bounds("connection-back").unwrap();
    visual.simulate_click(back.center(), Modifiers::default());
    draw(visual);
    assert!(visual.debug_bounds("connection-choice-0").is_some());
    visual.simulate_input("remote.test");
    visual.simulate_keystrokes("enter");
    draw(visual);
    visual.simulate_input(&tr("connection_action_edit"));
    draw(visual);
    visual.simulate_keystrokes("enter");
    draw(visual);
    assert!(
        matches!(*chosen.borrow(), Some(Choice::Action(Resource::Database(id), Action::Edit)) if id == target)
    );
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));

    visual.update(|window, cx| {
        let chosen = chosen.clone();
        open(Kind::Ssh, vec![], window, cx, move |choice, _, _| {
            *chosen.borrow_mut() = Some(choice)
        });
    });
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(visual);
    visual.simulate_keystrokes("enter");
    draw(visual);
    assert!(matches!(*chosen.borrow(), Some(Choice::Add)));
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
}
