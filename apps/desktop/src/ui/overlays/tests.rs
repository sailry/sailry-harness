use super::*;
use gpui_kit as gpui;
use gpui_kit::component::{
    Theme,
    button::Button,
    menu::{DropdownMenu, PopupMenuItem},
    popover::Popover,
    select::{SearchableVec, Select, SelectState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::test::{TestSupportExt as _, TestWindowExt as _};
use gpui_kit::{
    AbsoluteLength, AppContext, Bounds, Context, Corners, Entity, InteractiveElement, IntoElement,
    KeyDownEvent, KeyUpEvent, Keystroke, ParentElement, Render, Styled, TestAppContext,
    VisualTestContext, Window, div, point, rems,
};
use std::{cell::Cell, rc::Rc};

mod dialog;

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
    });
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.render_frame(cx));
}

fn activate(cx: &mut VisualTestContext) {
    let keystroke = Keystroke::parse("enter").unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: keystroke.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    cx.simulate_event(KeyUpEvent { keystroke });
    draw(cx);
}

pub(crate) fn panel(cx: &mut VisualTestContext, selector: &'static str) -> gpui_kit::Quad {
    let bounds = cx.debug_bounds(selector).expect("visible native surface");
    cx.update(|window, _| {
        // Match the renderer's edge snapping without changing logical layout assertions.
        let bounds = Bounds::from_corners(
            bounds.origin.map(|value| window.pixel_snap(value)),
            bounds.bottom_right().map(|value| window.pixel_snap(value)),
        )
        .scale(window.scale_factor());
        window
            .painted_quads()
            .into_iter()
            .find(|quad| quad.bounds == bounds && !quad.background.is_transparent())
            .expect("the native surface must paint a theme background")
    })
}

struct PopoverFixture {
    activations: Rc<Cell<usize>>,
    radius: Option<AbsoluteLength>,
}

impl Render for PopoverFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let activations = self.activations.clone();
        div().size_full().child(
            Popover::new("overlay-popover")
                .when_some(self.radius, |popover, radius| popover.rounded(radius))
                .trigger(Button::new("overlay-trigger").label("Open"))
                .content(move |_, _, _| {
                    let activations = activations.clone();
                    div().debug_selector(|| "overlay-content".into()).child(
                        Button::new("overlay-choice")
                            .label("Choose")
                            .on_click(move |_, _, _| {
                                activations.set(activations.get() + 1);
                            }),
                    )
                }),
        )
    }
}

#[gpui::test]
fn popover_corners_and_focus(cx: &mut TestAppContext) {
    setup(cx);
    let activations = Rc::new(Cell::new(0));
    let (_, visual) = cx.add_window_view({
        let activations = activations.clone();
        move |_, _| PopoverFixture {
            activations,
            radius: Some(rems(1.25).into()),
        }
    });
    draw(visual);
    let trigger = visual.update(|window, cx| {
        window.focus_next(cx);
        window.focused(cx).unwrap()
    });
    activate(visual);
    assert!(visual.debug_bounds("overlay-content").is_some());
    let quad = panel(visual, "popover-surface");
    let scale = visual.update(|window, _| window.scale_factor());
    assert_eq!(quad.corner_radii.top_left, px(20.).scale(scale));

    visual.update(|window, cx| window.focus_next(cx));
    activate(visual);
    assert_eq!(activations.get(), 1);
    assert!(visual.debug_bounds("overlay-content").is_some());

    visual.simulate_keystrokes("escape");
    draw(visual);
    assert!(visual.debug_bounds("overlay-content").is_none());
    assert!(visual.update(|window, _| trigger.is_focused(window)));
}

#[gpui::test]
fn popover_surface_radius(cx: &mut TestAppContext) {
    for (radius, expected) in [(8., 16.), (12., 24.), (0., 0.)] {
        setup(cx);
        cx.update(move |cx| Theme::global_mut(cx).radius_lg = px(radius));
        let (_, visual) = cx.add_window_view(|_, _| PopoverFixture {
            activations: Rc::new(Cell::new(0)),
            radius: None,
        });
        draw(visual);
        let trigger = visual.update(|window, cx| {
            window.focus_next(cx);
            window.focused(cx).unwrap()
        });
        activate(visual);
        assert!(visual.debug_bounds("overlay-content").is_some());
        let quad = panel(visual, "popover-surface");
        let scale = visual.update(|window, _| window.scale_factor());
        assert_eq!(quad.corner_radii, Corners::all(px(expected)).scale(scale));
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("overlay-content").is_none());
        assert!(visual.update(|window, _| trigger.is_focused(window)));
        visual.update(|window, _| window.remove_window());
    }
}

struct MenuFixture {
    activations: Rc<Cell<usize>>,
    nested: bool,
}

impl Render for MenuFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let activations = self.activations.clone();
        let nested = self.nested;
        div().size_full().child(
            Button::new("overlay-menu-trigger")
                .label("Open")
                .dropdown_menu(move |menu, window, cx| {
                    let activations = activations.clone();
                    let choice = PopupMenuItem::element(|_, _| {
                        div()
                            .debug_selector(|| "overlay-menu-label".into())
                            .line_height(gpui_kit::relative(1.))
                            .child("Choose")
                    })
                    .checked(true)
                    .on_click(move |_, _, _| activations.set(activations.get() + 1));
                    let adjacent = PopupMenuItem::element(|_, _| {
                        div()
                            .debug_selector(|| "overlay-menu-adjacent".into())
                            .line_height(gpui_kit::relative(1.))
                            .child("Another")
                    })
                    .disabled(true);
                    let disabled = || {
                        PopupMenuItem::new("Unavailable")
                            .disabled(true)
                            .on_click(|_, _, _| panic!("disabled menu item activated"))
                    };
                    let menu = menu.item(disabled());
                    if nested {
                        let submenu = PopupMenu::build(window, cx, |menu, _, _| {
                            menu.item(disabled())
                                .separator()
                                .item(choice)
                                .item(adjacent)
                        });
                        menu.item(PopupMenuItem::submenu("More", submenu))
                    } else {
                        menu.separator().item(choice).item(adjacent)
                    }
                }),
        )
    }
}

fn menu_choice(cx: &mut TestAppContext, nested: bool) {
    setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let activations = Rc::new(Cell::new(0));
    let (_, visual) = cx.add_window_view({
        let activations = activations.clone();
        move |_, _| MenuFixture {
            activations,
            nested,
        }
    });
    draw(visual);
    let trigger = visual.update(|window, cx| {
        window.focus_next(cx);
        window.focused(cx).unwrap()
    });
    activate(visual);
    assert!(!visual.update(|window, _| trigger.is_focused(window)));
    // Both initial selection and wrapping skip the disabled leading row.
    visual.simulate_keystrokes("down down");
    draw(visual);
    if nested {
        visual.simulate_keystrokes("right");
        draw(visual);
    }
    let quad = panel(visual, "popup-menu");
    let scale = visual.update(|window, _| window.scale_factor());
    assert_eq!(quad.corner_radii, Corners::all(px(16.)).scale(scale));
    assert_eq!(
        visual
            .debug_bounds("overlay-menu-label")
            .unwrap()
            .size
            .height,
        px(14.)
    );
    let choice = visual.debug_bounds("overlay-menu-label").unwrap();
    let adjacent = visual.debug_bounds("overlay-menu-adjacent").unwrap();
    // Each row has 6px vertical padding, with a real 1px gap between rows.
    assert_eq!(adjacent.top() - choice.bottom(), px(13.));
    activate(visual);
    assert_eq!(activations.get(), 1);
    assert!(visual.update(|window, _| trigger.is_focused(window)));
}

#[gpui::test]
fn menu_focus(cx: &mut TestAppContext) {
    menu_choice(cx, false);
}

#[gpui::test]
fn submenu_keyboard_navigation(cx: &mut TestAppContext) {
    menu_choice(cx, true);
}

#[derive(Clone)]
struct SelectOption(&'static str);

impl gpui_kit::component::select::SelectItem for SelectOption {
    type Value = &'static str;

    fn title(&self) -> gpui_kit::SharedString {
        self.0.into()
    }

    fn value(&self) -> &Self::Value {
        &self.0
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let label = self.0;
        div()
            .id(gpui_kit::SharedString::from(format!(
                "overlay-option-{label}"
            )))
            .test_support()
            .debug_selector(move || format!("overlay-option-{label}"))
            .line_height(gpui_kit::relative(1.))
            .child(label)
    }
}

struct SelectFixture {
    state: Entity<SelectState<SearchableVec<SelectOption>>>,
}

impl Render for SelectFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(px(40.)).child(
            div()
                .id("overlay-select")
                .test_support()
                .debug_selector(|| "overlay-select".into())
                .w(px(180.))
                .child(Select::new(&self.state).menu_width(px(240.)).empty(|_, _| {
                    div()
                        .debug_selector(|| "overlay-select-empty".into())
                        .w_full()
                        .h_5()
                        .child("Empty")
                })),
        )
    }
}

#[gpui::test]
fn select_bounds_and_motion(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| {
        let layout = cx.global::<PopupMenuLayout>();
        assert_eq!(layout.padding, px(8.));
        assert_eq!(layout.item_radius(cx.theme()), px(8.));
    });
    let (fixture, visual) = cx.add_window_view(|window, cx| SelectFixture {
        state: cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec![SelectOption("First"), SelectOption("Second")]),
                Some(gpui_kit::component::IndexPath::default()),
                window,
                cx,
            )
        }),
    });
    draw(visual);
    visual.update(|window, cx| window.click("overlay-select", cx));
    draw(visual);
    let opening = visual.update(|window, _| window.find("select-popup").bounds());
    assert_eq!(opening.size.width, px(240.));
    assert!(opening.size.height > px(0.));

    // Kit animations use the wall clock, independently of the test executor.
    std::thread::sleep(std::time::Duration::from_millis(600));
    draw(visual);
    let settled = visual.update(|window, _| window.find("select-popup").bounds());
    assert_eq!(opening.size, settled.size);
    let quad = panel(visual, "select-popup");
    let scale = visual.update(|window, _| window.scale_factor());
    assert_eq!(quad.corner_radii, Corners::all(px(16.)).scale(scale));
    let first = visual.update(|window, _| window.find("overlay-option-First").bounds());
    let second = visual.update(|window, _| window.find("overlay-option-Second").bounds());
    // Select retains its 4px row padding inside the shared 8px popup inset.
    assert_eq!(first.top() - settled.top(), px(12.));
    assert_eq!(settled.bottom() - second.bottom(), px(12.));
    // Kit Medium rows use 7px horizontal padding inside the shared 8px inset.
    assert_eq!(first.left() - settled.left(), px(15.));
    assert!(first.right() < settled.right() && second.right() < settled.right());
    assert_eq!(second.top() - first.bottom(), px(9.));
    assert!(
        opening.top() < settled.top(),
        "the popup should move with the entering menu: {opening:?} -> {settled:?}",
    );

    visual.update(|window, cx| window.press("escape", cx));
    draw(visual);
    assert!(visual.update(|window, _| window.try_find("select-popup").is_none()));
    visual.update(|window, cx| window.click("overlay-select", cx));
    draw(visual);
    let reopening = visual.update(|window, _| window.find("select-popup").bounds());
    assert_eq!(reopening.size, settled.size);
    assert!(
        reopening.top() < settled.top(),
        "the popup should restart its enter motion: {reopening:?} -> {settled:?}",
    );
    visual.update(|window, cx| {
        window.press("down", cx);
        window.press("enter", cx);
    });
    draw(visual);
    assert_eq!(
        visual.update(|_, cx| fixture.read(cx).state.read(cx).selected_value().copied()),
        Some("Second"),
    );
    assert!(visual.update(|window, _| window.try_find("select-popup").is_none()));
}

#[gpui::test]
fn select_surface_insets(cx: &mut TestAppContext) {
    setup(cx);
    cx.executor().allow_parking();
    for searchable in [false, true] {
        let (fixture, visual) = cx.add_window_view(|window, cx| SelectFixture {
            state: cx.new(|cx| {
                let items = if searchable {
                    vec![SelectOption("First"), SelectOption("Second")]
                } else {
                    vec![]
                };
                SelectState::new(SearchableVec::new(items), None, window, cx).searchable(searchable)
            }),
        });
        draw(visual);
        let trigger = visual.debug_bounds("overlay-select").unwrap().center();
        visual.simulate_click(trigger, Default::default());
        draw(visual);
        // Popup motion uses wall time; the search debounce uses the test clock.
        std::thread::sleep(std::time::Duration::from_millis(600));
        if searchable {
            visual.simulate_input("missing");
            visual.run_until_parked();
            visual
                .executor()
                .advance_clock(std::time::Duration::from_millis(150));
            visual.run_until_parked();
        }
        draw(visual);
        let popup = visual.debug_bounds("select-popup").unwrap();
        let empty = visual.debug_bounds("overlay-select-empty").unwrap();
        assert_eq!(empty.left() - popup.left(), px(8.));
        assert_eq!(popup.right() - empty.right(), px(8.));
        assert_eq!(popup.bottom() - empty.bottom(), px(8.));
        let content_top = empty.top() - popup.top();
        if searchable {
            let search = visual.debug_bounds("list-search").unwrap();
            // The selected Kit list adds 0.375rem above and below its 2rem input.
            let rem = visual.update(|window, _| window.rem_size());
            assert_eq!(search.size.height, rem * 2.75);
            assert_eq!(search.top(), popup.top());
            assert_eq!(empty.top(), search.bottom());
        } else {
            assert_eq!(empty.top() - popup.top(), px(8.));
        }
        if searchable {
            visual.simulate_keystrokes(
                "backspace backspace backspace backspace backspace backspace backspace",
            );
            visual.run_until_parked();
            visual
                .executor()
                .advance_clock(std::time::Duration::from_millis(150));
            visual.run_until_parked();
            draw(visual);
            let popup = visual.debug_bounds("select-popup").unwrap();
            let first = visual.debug_bounds("overlay-option-First").unwrap();
            let second = visual.debug_bounds("overlay-option-Second").unwrap();
            assert_eq!(first.top() - popup.top(), content_top + px(4.));
            assert_eq!(popup.bottom() - second.bottom(), px(12.));
            assert_eq!(second.top() - first.bottom(), px(9.));
            visual.simulate_keystrokes("down enter");
            visual.run_until_parked();
            draw(visual);
            assert!(
                visual.update(|_, cx| fixture.read(cx).state.read(cx).selected_value().is_some())
            );
        } else {
            visual.simulate_keystrokes("escape");
            draw(visual);
        }
        assert!(visual.debug_bounds("select-popup").is_none());
        visual.update(|window, _| window.remove_window());
    }
}
