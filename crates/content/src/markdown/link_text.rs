//! Shaped text with clickable link ranges.
//!
//! GPUI's `InteractiveText` is not used for links because it listens for
//! mouse-up only on a frame painted after mouse-down. A release that arrives
//! before that frame, such as a trackpad tap or a press during a slow redraw,
//! is lost and leaves the press pending, so the next click opens the link. This
//! element registers both listeners on every frame. Its shape follows
//! `InteractiveText` from gpui-fast 82d9177.
use gpui_kit::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, InspectorElementId, IntoElement, LayoutId, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, SharedString, StyledText, Window,
    accesskit,
};
use std::{cell::Cell, ops::Range, rc::Rc};

type OnClick = Box<dyn Fn(usize, Modifiers, &mut Window, &mut App)>;
type OnHover = Box<dyn Fn(Option<usize>, &mut Window, &mut App)>;

pub(super) struct LinkText {
    id: ElementId,
    text: StyledText,
    label: SharedString,
    ranges: Rc<[Range<usize>]>,
    on_click: Option<OnClick>,
    on_hover: Option<OnHover>,
}

#[derive(Default)]
struct Pointer {
    pressed: Rc<Cell<Option<usize>>>,
    hovered: Rc<Cell<Option<usize>>>,
}

impl LinkText {
    pub(super) fn new(
        id: impl Into<ElementId>,
        text: StyledText,
        label: SharedString,
        ranges: Vec<Range<usize>>,
        on_click: impl Fn(usize, Modifiers, &mut Window, &mut App) + 'static,
        on_hover: impl Fn(Option<usize>, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            text,
            label,
            ranges: ranges.into(),
            on_click: Some(Box::new(on_click)),
            on_hover: Some(Box::new(on_hover)),
        }
    }
}

/// The link holding a text index.
fn link_at(ranges: &[Range<usize>], index: usize) -> Option<usize> {
    ranges.iter().position(|range| range.contains(&index))
}

/// A click opens a link only when it is pressed and released on that link.
fn clicked(ranges: &[Range<usize>], pressed: Option<usize>, released: usize) -> Option<usize> {
    pressed.filter(|link| link_at(ranges, released) == Some(*link))
}

impl Element for LinkText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn a11y_role(&self) -> Option<accesskit::Role> {
        Some(accesskit::Role::Label)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        node.set_value(self.label.to_string());
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(None, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        self.text
            .prepaint(None, inspector_id, bounds, state, window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let view = window.current_view();
        let layout = self.text.layout().clone();
        let id = id.expect("LinkText always has an element id");
        window.with_element_state::<Pointer, _>(id, |state, window| {
            let state = state.unwrap_or_default();
            if layout
                .index_for_position(window.mouse_position())
                .is_ok_and(|index| link_at(&self.ranges, index).is_some())
            {
                window.set_cursor_style(CursorStyle::PointingHand, hitbox);
            }
            window.on_mouse_event({
                let (hitbox, layout) = (hitbox.clone(), layout.clone());
                let (ranges, pressed) = (self.ranges.clone(), state.pressed.clone());
                move |event: &MouseDownEvent, phase, window, _| {
                    if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
                        pressed.set(
                            layout
                                .index_for_position(event.position)
                                .ok()
                                .filter(|_| hitbox.is_hovered(window))
                                .and_then(|index| link_at(&ranges, index)),
                        );
                    }
                }
            });
            window.on_mouse_event({
                let (hitbox, layout) = (hitbox.clone(), layout.clone());
                let (ranges, pressed) = (self.ranges.clone(), state.pressed.clone());
                let on_click = self.on_click.take();
                move |event: &MouseUpEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }
                    // Taken before the hit test, so a release elsewhere leaves
                    // no press behind for the next click to complete.
                    let pressed = pressed.take();
                    if !hitbox.is_hovered(window) {
                        return;
                    }
                    let link = layout
                        .index_for_position(event.position)
                        .ok()
                        .and_then(|index| clicked(&ranges, pressed, index));
                    if let (Some(link), Some(on_click)) = (link, &on_click) {
                        on_click(link, event.modifiers, window, cx);
                    }
                }
            });
            window.on_mouse_event({
                let (hitbox, layout) = (hitbox.clone(), layout.clone());
                let hovered = state.hovered.clone();
                let on_hover = self.on_hover.take();
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble && hitbox.is_hovered(window) {
                        let index = layout.index_for_position(event.position).ok();
                        if hovered.replace(index) != index {
                            if let Some(on_hover) = &on_hover {
                                on_hover(index, window, cx);
                            }
                            cx.notify(view);
                        }
                    }
                }
            });
            self.text
                .paint(None, inspector_id, bounds, &mut (), &mut (), window, cx);
            ((), state)
        });
    }
}

impl IntoElement for LinkText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGES: [Range<usize>; 2] = [2..5, 7..9];

    #[test]
    fn finds_the_link_at_an_index() {
        assert_eq!(link_at(&RANGES, 2), Some(0));
        assert_eq!(link_at(&RANGES, 8), Some(1));
        assert_eq!(link_at(&RANGES, 5), None);
    }

    #[test]
    fn opens_when_press_and_release_match() {
        assert_eq!(clicked(&RANGES, Some(0), 3), Some(0));
        assert_eq!(clicked(&RANGES, Some(1), 7), Some(1));
    }

    #[test]
    fn ignores_a_release_off_the_pressed_link() {
        assert_eq!(clicked(&RANGES, Some(0), 7), None);
        assert_eq!(clicked(&RANGES, Some(0), 6), None);
        assert_eq!(clicked(&RANGES, None, 3), None);
    }
}
