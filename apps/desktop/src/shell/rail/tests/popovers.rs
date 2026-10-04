use super::*;
use std::{cell::Cell, rc::Rc};

// Observe Kit's outer surface through its existing renderer seam without adding
// a layout wrapper, changing the native menu, or retaining a rendered element.
struct Surface {
    child: AnyElement,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl Element for Surface {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.bounds.set(Some(bounds));
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
    }
}

impl IntoElement for Surface {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[gpui::test]
fn bottom_alignment_preserves_dismissal_and_clamping(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    let surface = Rc::new(Cell::new(None));
    visual.update({
        let surface = surface.clone();
        move |_, cx| {
            gpui_kit::component::surface::set_renderer(
                move |child, _, _| {
                    Surface {
                        child,
                        bounds: surface.clone(),
                    }
                    .into_any_element()
                },
                cx,
            );
        }
    });
    click(&mut visual, "composer-input");
    let focus = visual.update(|window, cx| window.focused(cx));
    assert!(focus.is_some());
    for (trigger, item) in [
        ("sidebar-host", "sidebar-host-option-0"),
        ("navigation-more", "more-navigation-files"),
    ] {
        surface.set(None);
        click(&mut visual, trigger);
        assert!(visual.debug_bounds(item).is_some());
        let panel = surface.get().expect("native rail menu surface");
        let trigger = visual.debug_bounds(trigger).unwrap();
        let rail = visual.debug_bounds("shell-feature-rail").unwrap();
        let offset = visual.update(|window, _| Shell::rail_popover_offset(window));
        assert!(panel.left() >= rail.right());
        assert!(panel.top() >= px(8.));
        assert!(
            f32::from(panel.bottom() - trigger.bottom()).abs() <= 1.,
            "rail menu bottom: panel={panel:?}, trigger={trigger:?}, offset={offset:?}"
        );
        assert_ne!(visual.update(|window, cx| window.focused(cx)), focus);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert!(visual.debug_bounds(item).is_none());
        assert_eq!(visual.update(|window, cx| window.focused(cx)), focus);
        assert_eq!(
            shell.read_with(&visual, |shell, _| shell.page),
            Page::Conversation
        );
    }

    let viewport = size(px(220.), px(480.));
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, viewport);
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    surface.set(None);
    click(&mut visual, "sidebar-host");
    let panel = surface.get().expect("clamped native rail menu surface");
    let trigger = visual.debug_bounds("sidebar-host").unwrap();
    assert!(panel.left() >= px(8.));
    assert!(panel.left() < visual.debug_bounds("shell-feature-rail").unwrap().right());
    assert!(panel.right() <= viewport.width - px(8.));
    assert!(panel.top() >= px(8.));
    assert!(panel.bottom() <= viewport.height - px(8.));
    assert!(f32::from(panel.bottom() - trigger.bottom()).abs() <= 1.);
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
    assert!(visual.debug_bounds("sidebar-host-option-0").is_none());
    assert_eq!(shell.read_with(&visual, |shell, _| shell.host), 0);
    assert!(shell.read_with(&visual, |shell, _| shell.live.is_none()));
}
