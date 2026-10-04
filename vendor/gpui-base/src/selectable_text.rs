use crate::LinkSpan;
use std::{cell::RefCell, ops::Range, rc::Rc};
type PressedLink = Rc<RefCell<Option<(LinkSpan, gpui::Point<Pixels>)>>>;
type LinkClick = Rc<dyn Fn(&LinkSpan, &mut Window, &mut App)>;

use gpui::{
    App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels, Point,
    SharedString, StyledText, TextStyleRefinement, Window, transparent_black,
};

use crate::{TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun};

/// Plain text that participates in the window-scoped [`TextSelection`].
///
/// Use [`Self::new`] for an independent run, or [`Self::with_handle`] when
/// several elements form one selectable document.
///
/// Applications must render one [`crate::TextSelectionLayer`] above their
/// content and call [`crate::init`] during startup. Selection and copy then
/// work without depending on `gpui-component`.
pub struct SelectableText {
    id: ElementId,
    handle: Option<TextSelectionHandle>,
    text: SharedString,
    styled_text: StyledText,
    document_order: u64,
    text_style: Option<TextStyleRefinement>,
    selection_color: Option<Hsla>,
    links: Vec<LinkSpan>,
    link_color: Option<Hsla>,
    on_link: Option<LinkClick>,
}

impl SelectableText {
    /// Creates a run that owns its own selection.
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self::build(id.into(), None, text.into())
    }

    /// Creates a run that joins the document `handle` belongs to.
    pub fn with_handle(
        id: impl Into<ElementId>,
        handle: TextSelectionHandle,
        text: impl Into<SharedString>,
    ) -> Self {
        Self::build(id.into(), Some(handle), text.into())
    }

    fn build(id: ElementId, handle: Option<TextSelectionHandle>, text: SharedString) -> Self {
        Self {
            id,
            handle,
            styled_text: StyledText::new(text.clone()),
            text,
            document_order: 0,
            text_style: None,
            selection_color: None,
            links: Vec::new(),
            link_color: None,
            on_link: None,
        }
    }

    /// Places this run in reading order among the others sharing its handle.
    pub fn document_order(mut self, order: u64) -> Self {
        self.document_order = order;
        self
    }

    /// Sets the text style the run is laid out and painted with.
    pub fn text_style(mut self, style: TextStyleRefinement) -> Self {
        self.text_style = Some(style);
        self
    }

    /// Overrides the selection background, which defaults to the theme's
    /// `colors.selection` token.
    pub fn selection_color(mut self, color: Hsla) -> Self {
        self.selection_color = Some(color);
        self
    }

    /// Decorates existing source spans without changing plain-text selection or copy.
    pub fn links(
        mut self,
        links: Vec<LinkSpan>,
        color: Hsla,
        listener: impl Fn(&LinkSpan, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.links = links
            .into_iter()
            .filter(|link| {
                link.range.start < link.range.end && self.text.get(link.range.clone()).is_some()
            })
            .collect();
        self.link_color = Some(color);
        self.on_link = Some(Rc::new(listener));
        let highlights = gpui::combine_highlights(
            self.links.iter().map(|link| {
                (
                    link.range.clone(),
                    gpui::HighlightStyle {
                        color: Some(color),
                        ..Default::default()
                    },
                )
            }),
            self.links
                .iter()
                .filter(|link| link.icon.is_some())
                .filter_map(|link| {
                    Some((
                        link.marker_range(&self.text)?,
                        gpui::HighlightStyle {
                            fade_out: Some(1.),
                            ..Default::default()
                        },
                    ))
                }),
        );
        self.styled_text = StyledText::new(self.text.clone()).with_highlights(highlights);
        self
    }

    fn paint_selection(
        layout: &gpui::TextLayout,
        range: Range<usize>,
        color: Hsla,
        window: &mut Window,
    ) {
        let (Some(start), Some(end)) = (
            layout.position_for_index(range.start),
            layout.position_for_index(range.end),
        ) else {
            return;
        };
        for bounds in selection_quad_bounds(start, end, layout.bounds(), layout.line_height()) {
            window.paint_quad(PaintQuad {
                bounds,
                background: color.into(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                border_color: transparent_black(),
                border_style: BorderStyle::default(),
            });
        }
    }
}

fn selection_quad_bounds(
    start: Point<Pixels>,
    end: Point<Pixels>,
    bounds: Bounds<Pixels>,
    line_height: Pixels,
) -> Vec<Bounds<Pixels>> {
    if start.y == end.y {
        return vec![Bounds::from_corners(
            start,
            Point::new(end.x, end.y + line_height),
        )];
    }

    let mut quads = vec![Bounds::from_corners(
        start,
        Point::new(bounds.right(), start.y + line_height),
    )];
    if end.y > start.y + line_height {
        quads.push(Bounds::from_corners(
            Point::new(bounds.left(), start.y + line_height),
            Point::new(bounds.right(), end.y),
        ));
    }
    quads.push(Bounds::from_corners(
        Point::new(bounds.left(), end.y),
        Point::new(end.x, end.y + line_height),
    ));
    quads
}

impl IntoElement for SelectableText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableText {
    type RequestLayoutState = (TextSelectionHandle, PressedLink);
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let retained = window.with_element_state(
            global_id.expect("SelectableText must have a stable element id"),
            |retained: Option<(TextSelectionHandle, PressedLink)>, _| {
                let mut retained = retained.unwrap_or_else(|| {
                    (
                        self.handle
                            .clone()
                            .unwrap_or_else(|| TextSelectionHandle::new(self.text.clone(), cx)),
                        Rc::new(RefCell::new(None)),
                    )
                });
                if let Some(handle) = &self.handle {
                    retained.0 = handle.clone();
                }
                (retained.clone(), retained)
            },
        );
        let (layout_id, ()) = if let Some(style) = self.text_style.clone() {
            window.with_text_style(Some(style), |window| {
                self.styled_text
                    .request_layout(global_id, inspector_id, window, cx)
            })
        } else {
            self.styled_text
                .request_layout(global_id, inspector_id, window, cx)
        };
        (layout_id, retained)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_document_order(self.document_order)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(&handle.0, window, cx);
        handle.0.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        let selected_text_before = TextSelection::selected_text(window, cx);
        let projection = handle.0.update_runs(
            &[
                TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)
                    .with_document_order(self.document_order),
            ],
            cx,
        );
        if selected_text_before != TextSelection::selected_text(window, cx) {
            window.refresh();
        }
        let color = self
            .selection_color
            .unwrap_or_else(|| crate::Theme::global(cx).tokens.colors.selection);
        for range in projection.ranges().iter().flatten().cloned() {
            Self::paint_selection(&layout, range, color, window);
        }
        self.styled_text.paint(
            global_id,
            inspector_id,
            bounds,
            &mut (),
            &mut (),
            window,
            cx,
        );
        let links = Rc::new(self.links.clone());
        let color = self.link_color.unwrap_or_else(|| window.text_style().color);
        for link in links.iter().filter(|link| link.icon.is_some()) {
            let Some(marker) = link.marker_range(&self.text) else {
                continue;
            };
            if let (Some(start), Some(end)) = (
                layout.position_for_index(marker.start),
                layout.position_for_index(marker.end),
            ) {
                link.paint_icon(
                    Bounds::from_corners(start, gpui::point(end.x, start.y + layout.line_height())),
                    color,
                    window,
                    cx,
                );
            }
        }
        if let Some(listener) = self.on_link.clone() {
            if layout
                .index_for_position(window.mouse_position())
                .ok()
                .is_some_and(|offset| links.iter().any(|link| link.range.contains(&offset)))
            {
                window.set_cursor_style(gpui::CursorStyle::PointingHand, hitbox);
            }
            let pressed = handle.1.clone();
            let down_links = links.clone();
            let down_layout = layout.clone();
            let down_hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, _| {
                if phase.bubble() {
                    *pressed.borrow_mut() = if down_hitbox.is_hovered(window)
                        && event.button == gpui::MouseButton::Left
                        && event.click_count == 1
                        && !event.modifiers.modified()
                    {
                        down_layout
                            .index_for_position(event.position)
                            .ok()
                            .and_then(|offset| {
                                down_links
                                    .iter()
                                    .find(|link| link.range.contains(&offset))
                                    .cloned()
                            })
                            .map(|link| (link, event.position))
                    } else {
                        None
                    };
                }
            });
            let pressed = handle.1.clone();
            window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, _| {
                if phase.bubble() {
                    let cancel = pressed.borrow().as_ref().is_some_and(|(_, origin)| {
                        let delta = event.position - *origin;
                        delta.x.abs() > gpui::px(3.) || delta.y.abs() > gpui::px(3.)
                    });
                    if cancel {
                        *pressed.borrow_mut() = None;
                    }
                }
            });
            let pressed = handle.1.clone();
            let hitbox = hitbox.clone();
            window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, window, cx| {
                if !phase.bubble() {
                    return;
                }
                let Some((link, origin)) = pressed.borrow_mut().take() else {
                    return;
                };
                let delta = event.position - origin;
                if event.button != gpui::MouseButton::Left
                    || event.modifiers.modified()
                    || delta.x.abs() > gpui::px(3.)
                    || delta.y.abs() > gpui::px(3.)
                    || TextSelection::has_selection(window, cx)
                    || !hitbox.is_hovered(window)
                    || !links.contains(&link)
                {
                    return;
                }
                if layout
                    .index_for_position(event.position)
                    .ok()
                    .is_some_and(|offset| link.range.contains(&offset))
                {
                    listener(&link, window, cx);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        Bounds, Context, IntoElement, Modifiers, MouseButton, ParentElement as _, Render,
        Styled as _, TestAppContext, Window, div, point, px, size,
    };

    use super::SelectableText;
    use crate::{TextSelection, TextSelectionHandle, TextSelectionLayer};

    struct SelectableTextTestView;

    impl Render for SelectableTextTestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(TextSelectionLayer).child(
                div()
                    .w(px(240.))
                    .h(px(32.))
                    .child(SelectableText::new("local", "alpha beta")),
            )
        }
    }

    #[gpui::test]
    fn explicit_handle_constructor_preserves_document_contract(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let handle = TextSelectionHandle::new("alpha beta", cx);
            let _ = SelectableText::with_handle("plain", handle, "alpha beta").document_order(42);
        });
    }

    #[gpui::test]
    fn local_handle_participates_in_window_selection(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| SelectableTextTestView);
        cx.update(|window, cx| {
            let _ = window.draw(cx);
        });

        cx.simulate_mouse_down(
            gpui::point(px(1.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_move(
            gpui::point(px(220.), px(12.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            gpui::point(px(220.), px(12.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.update(|window, cx| {
            let _ = window.draw(cx);
            assert_eq!(TextSelection::selected_text(window, cx), "alpha beta");
        });
    }

    #[test]
    fn wrapped_selection_paints_full_width_middle_lines() {
        let bounds = Bounds::new(point(px(10.), px(20.)), size(px(100.), px(100.)));
        let quads = super::selection_quad_bounds(
            point(px(40.), px(20.)),
            point(px(30.), px(80.)),
            bounds,
            px(20.),
        );

        assert_eq!(
            quads,
            vec![
                Bounds::from_corners(point(px(40.), px(20.)), point(px(110.), px(40.))),
                Bounds::from_corners(point(px(10.), px(40.)), point(px(110.), px(80.))),
                Bounds::from_corners(point(px(10.), px(80.)), point(px(30.), px(100.))),
            ]
        );
    }
}
