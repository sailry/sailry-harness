use super::*;
use gpui_kit as gpui;
use gpui_kit::component::{
    Theme, ThemeMode,
    button::Button,
    menu::{DropdownMenu, PopupMenuItem},
    popover::Popover,
    select::{SearchableVec, Select, SelectState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext, Context, Entity, InteractiveElement, KeyDownEvent, KeyUpEvent, Keystroke,
    ParentElement, Render, Styled, TestAppContext, VisualTestContext, div, point, rems,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

mod dialog;
mod layer;

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        init(cx);
    });
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
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

#[test]
fn corner_units_and_limits() {
    let surface = Surface {
        child: div().into_any_element(),
        corners: Corners {
            top_left: px(4.).into(),
            top_right: rems(1.).into(),
            bottom_right: px(30.).into(),
            bottom_left: rems(1.5).into(),
        },
    };
    let corners = surface.corners(
        Bounds::new(point(px(7.), px(9.)), size(px(60.), px(40.))),
        px(16.),
    );
    assert_eq!(corners.top_left, px(4.));
    assert_eq!(corners.top_right, px(16.));
    assert_eq!(corners.bottom_right, px(20.));
    assert_eq!(corners.bottom_left, px(20.));
}

#[test]
fn glass_contrast() {
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        let mut theme = Theme {
            mode,
            ..Default::default()
        };
        theme.background = gpui_kit::rgb(0x0000ff).into();
        theme.popover = gpui_kit::rgb(0xff0000).into();
        for (alpha, expected_red, expected_blue) in [(1., 1., 0.), (0.75, 0.75, 0.25)] {
            theme.popover.a = alpha;
            let effect = glass(&theme, size(px(240.), px(160.)));
            let tint = effect.tint.to_rgb();
            assert!((effect.gain - 0.10).abs() < 1e-6);
            assert!((tint.a - 0.90).abs() < 1e-6);
            assert!((tint.r - expected_red).abs() < 1e-6);
            assert!(tint.g.abs() < 1e-6);
            assert!((tint.b - expected_blue).abs() < 1e-6);
            assert!(effect.blur_radius > px(0.));
            assert!(effect.lens > px(0.));
            assert!(effect.reach > px(0.));
            assert!(effect.dispersion > 0.);
        }
    }
}

struct PopoverFixture {
    activations: Rc<Cell<usize>>,
    radius: Option<AbsoluteLength>,
}

impl Render for PopoverFixture {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let activations = self.activations.clone();
        div().size_full().child(
            Popover::new("material-popover")
                .when_some(self.radius, |popover, radius| popover.rounded(radius))
                .trigger(Button::new("material-trigger").label("Open"))
                .content(move |_, _, _| {
                    let activations = activations.clone();
                    div().debug_selector(|| "material-content".into()).child(
                        Button::new("material-choice")
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
    let corners = Rc::new(RefCell::new(Vec::new()));
    cx.update({
        let captured = corners.clone();
        move |cx| {
            gpui_kit::component::surface::set_renderer(
                move |child, corners, _| {
                    captured.borrow_mut().push(corners);
                    Surface { child, corners }.into_any_element()
                },
                cx,
            )
        }
    });
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
    assert!(visual.debug_bounds("material-content").is_some());
    assert!(!corners.borrow().is_empty());
    assert!(
        corners
            .borrow()
            .iter()
            .all(|corners| corners.top_left == rems(1.25).into())
    );

    visual.update(|window, cx| window.focus_next(cx));
    activate(visual);
    assert_eq!(activations.get(), 1);
    assert!(visual.debug_bounds("material-content").is_some());

    visual.simulate_keystrokes("escape");
    draw(visual);
    assert!(visual.debug_bounds("material-content").is_none());
    assert!(visual.update(|window, _| trigger.is_focused(window)));
}

#[gpui::test]
fn popover_surface_radius(cx: &mut TestAppContext) {
    for (radius, expected) in [(8., 16.), (12., 24.), (0., 0.)] {
        setup(cx);
        cx.update(move |cx| {
            Theme::global_mut(cx).radius_lg = px(radius);
            gpui_kit::component::surface::set_renderer(
                move |child, corners, _| {
                    assert_eq!(
                        corners,
                        Corners::all(px(expected)).map(|radius| (*radius).into())
                    );
                    Surface { child, corners }.into_any_element()
                },
                cx,
            );
        });
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
        assert!(visual.debug_bounds("material-content").is_some());
        visual.simulate_keystrokes("escape");
        draw(visual);
        assert!(visual.debug_bounds("material-content").is_none());
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
            Button::new("material-menu-trigger")
                .label("Open")
                .dropdown_menu(move |menu, window, cx| {
                    let activations = activations.clone();
                    let choice = PopupMenuItem::element(|_, _| {
                        div()
                            .debug_selector(|| "material-menu-label".into())
                            .line_height(gpui_kit::relative(1.))
                            .child("Choose")
                    })
                    .checked(true)
                    .on_click(move |_, _, _| activations.set(activations.get() + 1));
                    let adjacent = PopupMenuItem::element(|_, _| {
                        div()
                            .debug_selector(|| "material-menu-adjacent".into())
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
    cx.update(|cx| {
        gpui_kit::component::surface::set_renderer(
            |child, corners, _| {
                assert_eq!(
                    corners,
                    Corners::all(px(16.)).map(|radius| (*radius).into())
                );
                Surface { child, corners }.into_any_element()
            },
            cx,
        );
    });
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
    assert_eq!(
        visual
            .debug_bounds("material-menu-label")
            .unwrap()
            .size
            .height,
        px(14.)
    );
    let choice = visual.debug_bounds("material-menu-label").unwrap();
    let adjacent = visual.debug_bounds("material-menu-adjacent").unwrap();
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
            .debug_selector(move || format!("material-option-{label}"))
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
                .debug_selector(|| "material-select".into())
                .w(px(180.))
                .child(Select::new(&self.state).menu_width(px(240.)).empty(|_, _| {
                    div()
                        .debug_selector(|| "material-select-empty".into())
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
        gpui_kit::component::surface::set_renderer(
            |child, corners, _| {
                assert_eq!(
                    corners,
                    Corners::all(px(16.)).map(|radius| (*radius).into())
                );
                div()
                    .debug_selector(|| "material-dropdown".into())
                    .child(Surface { child, corners })
                    .into_any_element()
            },
            cx,
        );
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
    let trigger = visual.debug_bounds("material-select").unwrap().center();
    visual.simulate_click(trigger, Default::default());
    draw(visual);
    let opening = visual.debug_bounds("material-dropdown").unwrap();
    assert_eq!(opening.size.width, px(240.));
    assert!(opening.size.height > px(0.));

    // Kit animations use the wall clock, independently of the test executor.
    std::thread::sleep(std::time::Duration::from_millis(600));
    draw(visual);
    let settled = visual.debug_bounds("material-dropdown").unwrap();
    assert_eq!(opening.size, settled.size);
    let first = visual.debug_bounds("material-option-First").unwrap();
    let second = visual.debug_bounds("material-option-Second").unwrap();
    // Select retains its 4px row padding inside the shared 8px popup inset.
    assert_eq!(first.top() - settled.top(), px(12.));
    assert_eq!(settled.bottom() - second.bottom(), px(12.));
    assert_eq!(first.left() - settled.left(), px(20.));
    assert_eq!(second.top() - first.bottom(), px(9.));
    assert!(
        opening.top() < settled.top(),
        "the material should move with the entering menu: {opening:?} -> {settled:?}",
    );

    visual.simulate_keystrokes("escape");
    draw(visual);
    assert!(visual.debug_bounds("material-dropdown").is_none());
    visual.simulate_click(trigger, Default::default());
    draw(visual);
    let reopening = visual.debug_bounds("material-dropdown").unwrap();
    assert_eq!(reopening.size, settled.size);
    assert!(
        reopening.top() < settled.top(),
        "the whole material should restart its enter motion: {reopening:?} -> {settled:?}",
    );
    visual.simulate_keystrokes("down enter");
    draw(visual);
    assert_eq!(
        visual.update(|_, cx| fixture.read(cx).state.read(cx).selected_value().copied()),
        Some("Second"),
    );
    assert!(visual.debug_bounds("material-dropdown").is_none());
}

#[gpui::test]
fn select_surface_insets(cx: &mut TestAppContext) {
    setup(cx);
    cx.executor().allow_parking();
    cx.update(|cx| {
        gpui_kit::component::surface::set_renderer(
            |child, corners, _| {
                div()
                    .debug_selector(|| "material-dropdown".into())
                    .child(Surface { child, corners })
                    .into_any_element()
            },
            cx,
        );
    });
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
        let trigger = visual.debug_bounds("material-select").unwrap().center();
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
        let popup = visual.debug_bounds("material-dropdown").unwrap();
        let empty = visual.debug_bounds("material-select-empty").unwrap();
        assert_eq!(empty.left() - popup.left(), px(8.));
        assert_eq!(popup.right() - empty.right(), px(8.));
        assert_eq!(popup.bottom() - empty.bottom(), px(8.));
        let content_top = empty.top() - popup.top();
        if searchable {
            let search = visual.debug_bounds("list-search").unwrap();
            // The selected Kit list adds 0.375rem above and below its 2rem input.
            let rem = visual.update(|window, _| window.rem_size());
            assert_eq!(search.size.height, rem * 2.75 + px(1.));
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
            let popup = visual.debug_bounds("material-dropdown").unwrap();
            let first = visual.debug_bounds("material-option-First").unwrap();
            let second = visual.debug_bounds("material-option-Second").unwrap();
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
        assert!(visual.debug_bounds("material-dropdown").is_none());
        visual.update(|window, _| window.remove_window());
    }
}
