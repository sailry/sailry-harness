use super::*;
use core::prelude::v1::test;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::menu::DropdownMenu;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_reduce_motion(true);
    });
}

struct Welcome {
    selected: std::rc::Rc<std::cell::RefCell<Vec<SharedString>>>,
    input: Entity<TextareaState>,
}

impl Render for Welcome {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.selected.clone();
        let menu_selected = self.selected.clone();
        crate::conversation::welcome::content(
            std::rc::Rc::new(move |prompt, _, _| selected.borrow_mut().push(prompt)),
            v_flex()
                .child(Textarea::new(&self.input))
                .child(
                    Button::new("welcome-permission")
                        .self_start()
                        .label(tr("composer_permission_ask"))
                        .debug_selector(|| "welcome-permission".into())
                        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
                            crate::conversation::permission::MODES.into_iter().fold(
                                menu,
                                |menu, mode| {
                                    let selected = menu_selected.clone();
                                    menu.item(
                                        crate::conversation::permission::item(mode, false)
                                            .on_click(move |_, _, _| {
                                                selected.borrow_mut().push(tr(
                                                    crate::conversation::permission::label(mode),
                                                ));
                                            }),
                                    )
                                },
                            )
                        }),
                )
                .into_any_element(),
            cx,
        )
    }
}

type Core = (Bounds<ScaledPixels>, Hsla);

fn cores(visual: &mut VisualTestContext) -> Vec<Core> {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    let dots = visual.debug_bounds("welcome-dots").unwrap();
    visual.update(|window, _| {
        let scale = window.scale_factor();
        let bounds = dots.scale(scale);
        let edge = ScaledPixels(f32::from(dots.size.height) / 112. * 6. * scale + 1.);
        let mut cores: Vec<_> = window
            .painted_quads()
            .into_iter()
            .filter(|quad| {
                quad.bounds.left() >= bounds.left()
                    && quad.bounds.right() <= bounds.right()
                    && quad.bounds.top() >= bounds.top()
                    && quad.bounds.bottom() <= bounds.bottom()
                    && quad.bounds.size.width <= edge
                    && quad.bounds.size.height <= edge
            })
            .map(|quad| (quad.bounds, quad.background.as_solid().unwrap()))
            .collect();
        cores.sort_by_key(|(bounds, _)| (bounds.left(), bounds.top()));
        assert_eq!(cores.len(), 214);
        cores
    })
}

fn brightest(cores: &[Core]) -> ScaledPixels {
    cores
        .iter()
        .max_by(|(_, left), (_, right)| left.a.total_cmp(&right.a))
        .unwrap()
        .0
        .center()
        .x
}

#[gpui_kit::test]
fn retains_pointer_on_typing_and_window_exit(cx: &mut TestAppContext) {
    init(cx);
    let selected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let window = cx.open_window(size(px(760.), px(820.)), |window, cx| Welcome {
        selected: selected.clone(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    let view = window.root(cx).unwrap();
    let visual = &mut VisualTestContext::from_window(*window, cx);
    let input = view.read_with(visual, |view, _| view.input.clone());
    visual.update(|window, cx| input.update(cx, |input, cx| input.focus(window, cx)));
    let centered = cores(visual);
    let dots = visual.debug_bounds("welcome-dots").unwrap();
    let left = point(dots.left() + dots.size.width * 0.15, dots.center().y);
    let right = point(dots.left() + dots.size.width * 0.85, dots.center().y);
    visual.simulate_mouse_move(left, None, Modifiers::default());
    let left_cores = cores(visual);
    assert_ne!(left_cores, centered);
    visual.simulate_mouse_move(
        point(left.x, dots.top() + px(2.)),
        None,
        Modifiers::default(),
    );
    assert_eq!(cores(visual), left_cores);
    visual.simulate_mouse_move(
        point(left.x, dots.bottom() - px(2.)),
        None,
        Modifiers::default(),
    );
    assert_eq!(cores(visual), left_cores);

    visual.simulate_input("x");
    assert_eq!(cores(visual), left_cores);
    assert_eq!(input.read_with(visual, |input, _| input.value()), "x");
    assert!(visual.update(|window, cx| input.focus_handle(cx).is_focused(window)));

    visual.simulate_mouse_move(right, None, Modifiers::default());
    let right_cores = cores(visual);
    assert!(brightest(&left_cores) < brightest(&centered));
    assert!(brightest(&centered) < brightest(&right_cores));
    assert_eq!(
        left_cores
            .iter()
            .map(|(bounds, _)| bounds)
            .collect::<Vec<_>>(),
        right_cores
            .iter()
            .map(|(bounds, _)| bounds)
            .collect::<Vec<_>>()
    );
    visual.simulate_event(MouseExitEvent {
        position: right,
        ..Default::default()
    });
    assert_eq!(cores(visual), right_cores);
    visual.simulate_mouse_move(point(px(800.), right.y), None, Modifiers::default());
    assert_eq!(cores(visual), right_cores);
    visual.simulate_mouse_move(left, None, Modifiers::default());
    assert_eq!(cores(visual), left_cores);
    assert!(selected.borrow().is_empty());
    assert_eq!(input.read_with(visual, |input, _| input.value()), "x");
}

#[gpui_kit::test]
fn retains_lighting_across_themes_and_sizes(cx: &mut TestAppContext) {
    init(cx);
    let window = cx.open_window(size(px(760.), px(820.)), |window, cx| Welcome {
        selected: Default::default(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    let visual = &mut VisualTestContext::from_window(*window, cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| Theme::change(mode, Some(window), cx));
        for width in [120., 280., 760.] {
            visual.simulate_event(MouseExitEvent::default());
            visual.simulate_window_resize(*window, size(px(width), px(820.)));
            cores(visual);
            let dots = visual.debug_bounds("welcome-dots").unwrap();
            let left = point(dots.left() + dots.size.width * 0.15, dots.center().y);
            let right = point(dots.left() + dots.size.width * 0.85, dots.center().y);
            visual.simulate_mouse_move(left, None, Modifiers::default());
            let left_cores = cores(visual);
            visual.simulate_mouse_move(right, None, Modifiers::default());
            let right_cores = cores(visual);
            assert!(brightest(&left_cores) < brightest(&right_cores));
            assert_eq!(visual.debug_bounds("welcome-dots"), Some(dots));
            let ink = visual.update(|_, cx| cx.theme().foreground);
            for (_, color) in &right_cores {
                assert_eq!((color.h, color.s, color.l), (ink.h, ink.s, ink.l));
                assert!(color.a.is_finite() && color.a > 0. && color.a <= ink.a);
            }
            visual.simulate_event(MouseExitEvent {
                position: right,
                ..Default::default()
            });
            assert_eq!(cores(visual), right_cores);
        }
    }
}

#[gpui_kit::test]
fn starts_centered_on_remount(cx: &mut TestAppContext) {
    init(cx);
    let window = cx.open_window(size(px(760.), px(820.)), |window, cx| Welcome {
        selected: Default::default(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    let visual = &mut VisualTestContext::from_window(*window, cx);
    let centered = cores(visual);
    let dots = visual.debug_bounds("welcome-dots").unwrap();
    visual.simulate_mouse_move(
        point(dots.left() + dots.size.width * 0.15, dots.center().y),
        None,
        Modifiers::default(),
    );
    assert_ne!(cores(visual), centered);
    visual.replace_root_view(|window, cx| Welcome {
        selected: Default::default(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    assert_eq!(cores(visual), centered);
}

#[gpui_kit::test]
fn retains_pointer_under_menu_hover(cx: &mut TestAppContext) {
    init(cx);
    let selected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let window = cx.open_window(size(px(760.), px(820.)), |window, cx| Welcome {
        selected: selected.clone(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    let visual = &mut VisualTestContext::from_window(*window, cx);
    let centered = cores(visual);
    let trigger_bounds = visual.debug_bounds("welcome-permission").unwrap();
    let trigger = point(trigger_bounds.left() + px(4.), trigger_bounds.center().y);
    visual.simulate_mouse_move(trigger, None, Modifiers::default());
    let last = cores(visual);
    assert_ne!(last, centered);
    visual.simulate_click(trigger, Modifiers::default());
    assert_eq!(cores(visual), last);
    let mut second = trigger;
    for selector in [
        "composer_permission_ask-option",
        "composer_permission_project-option",
    ] {
        let bounds = visual.debug_bounds(selector).unwrap();
        second = bounds.center();
        visual.simulate_mouse_move(second, None, Modifiers::default());
        assert_eq!(cores(visual), last);
    }
    visual.simulate_click(second, Modifiers::default());
    assert_eq!(cores(visual), last);
    assert_eq!(&*selected.borrow(), &[tr("composer_permission_project")]);
    assert!(
        visual
            .debug_bounds("composer_permission_project-option")
            .is_none()
    );
    let dots = visual.debug_bounds("welcome-dots").unwrap();
    visual.simulate_mouse_move(
        point(dots.right(), dots.center().y),
        None,
        Modifiers::default(),
    );
    assert_ne!(cores(visual), last);
}

#[gpui_kit::test]
fn preserves_prompt_actions(cx: &mut TestAppContext) {
    init(cx);
    let selected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let window = cx.open_window(size(px(760.), px(820.)), |window, cx| Welcome {
        selected: selected.clone(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    let visual = &mut VisualTestContext::from_window(*window, cx);
    visual.run_until_parked();
    assert!(visual.debug_bounds("welcome-mark").is_none());
    assert!(visual.debug_bounds("welcome-hi").is_none());
    let wordmark = visual.debug_bounds("welcome-wordmark").unwrap();
    let dots = visual.debug_bounds("welcome-dots").unwrap();
    assert_eq!(dots.size.height, px(72.));
    let scale = visual.update(|window, _| window.scale_factor());
    assert_eq!(
        dots.size.width,
        px((72. * wordmark::RATIO * scale).round() / scale),
    );
    assert!(wordmark.contains(&dots.origin));
    for action in [
        "welcome_explore",
        "welcome_build",
        "welcome_review",
        "welcome_plan",
    ] {
        let bounds = visual.debug_bounds(action).unwrap();
        visual.simulate_click(bounds.center(), Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            selected.borrow().last(),
            Some(&crate::tr(&format!("{action}_prompt")))
        );
    }
    assert_eq!(selected.borrow().len(), 4);
}

#[gpui_kit::test]
fn narrow_pane_bounds(cx: &mut TestAppContext) {
    init(cx);
    let window = cx.open_window(size(px(180.), px(820.)), |window, cx| Welcome {
        selected: Default::default(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    let visual = &mut VisualTestContext::from_window(*window, cx);
    for (width, logo_size) in [
        (180., 24.),
        (120., 20.),
        (280., 32.),
        (320., 66.),
        (360., 72.),
    ] {
        visual.simulate_window_resize(*window, size(px(width), px(820.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds("welcome-hi").is_none());
        for selector in [
            "welcome-wordmark",
            "welcome-dots",
            "welcome_explore",
            "welcome_build",
            "welcome_review",
            "welcome_plan",
        ] {
            let bounds = visual.debug_bounds(selector).unwrap();
            assert!(bounds.left() >= px(0.), "{selector}");
            assert!(bounds.right() <= px(width), "{selector}");
        }
        let dots = visual.debug_bounds("welcome-dots").unwrap();
        assert_eq!(dots.size.height, px(logo_size));
        // GPUI snaps authored lengths to the window's device-pixel grid.
        let scale = visual.update(|window, _| window.scale_factor());
        let expected = (logo_size * wordmark::RATIO * scale).round() / scale;
        assert_eq!(dots.size.width, px(expected));
    }
}

#[gpui_kit::test]
fn theme_brand_fits_with_wordmark(cx: &mut TestAppContext) {
    init(cx);
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "welcome");
    let package = crate::theme::package::Loaded::read(&source, true).unwrap();
    cx.update(|cx| {
        let catalog = cx.global_mut::<crate::theme::Catalog>();
        catalog.packages.push(package.into());
        catalog.selected = catalog.packages.len() - 1;
    });
    let window = cx.open_window(size(px(760.), px(820.)), |window, cx| Welcome {
        selected: Default::default(),
        input: cx.new(|cx| TextareaState::new(window, cx)),
    });
    let visual = &mut VisualTestContext::from_window(*window, cx);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        visual.update(|window, cx| Theme::change(mode, Some(window), cx));
        for width in [120., 280., 320., 760.] {
            visual.simulate_window_resize(*window, size(px(width), px(820.)));
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let wordmark = visual.debug_bounds("welcome-wordmark").unwrap();
            let brand = visual.debug_bounds("welcome-brand").unwrap();
            let dots = visual.debug_bounds("welcome-dots").unwrap();
            assert!(wordmark.left() >= px(0.) && wordmark.right() <= px(width));
            assert!(brand.left() >= wordmark.left() && brand.right() <= wordmark.right());
            assert!(dots.left() >= wordmark.left() && dots.right() <= wordmark.right());
            assert_eq!(brand.size.height, dots.size.height);
            assert_eq!(
                dots.left() - brand.right(),
                visual.update(|window, _| window.rem_size() * 0.75),
            );
            assert!(visual.debug_bounds("theme-hero.new_session").is_some());
        }
    }
}
