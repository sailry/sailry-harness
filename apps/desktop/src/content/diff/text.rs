//! Records only mounted row layouts for precise pointer selection.
use super::selection::Cursor;
use gpui_kit::*;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

type Rows = BTreeMap<usize, (TextLayout, Bounds<Pixels>)>;

#[derive(Clone, Default)]
pub(super) struct Layouts(pub Rc<RefCell<Rows>>);

impl Layouts {
    pub fn hit(&self, position: Point<Pixels>) -> Option<Cursor> {
        let layouts = self.0.borrow();
        let (&row, (layout, bounds)) = layouts.iter().min_by(|(_, (_, a)), (_, (_, b))| {
            let distance = |bounds: &Bounds<Pixels>| {
                if position.y < bounds.top() {
                    bounds.top() - position.y
                } else if position.y > bounds.bottom() {
                    position.y - bounds.bottom()
                } else {
                    px(0.)
                }
            };
            distance(a)
                .partial_cmp(&distance(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;
        let point = point(
            position.x,
            position.y.clamp(bounds.top(), bounds.bottom() - px(0.01)),
        );
        let byte = layout.index_for_position(point).unwrap_or_else(|byte| byte);
        Some(Cursor { row, byte })
    }
}

pub(super) struct Text {
    pub text: StyledText,
    pub row: usize,
    pub layouts: Layouts,
}

impl IntoElement for Text {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Text {
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
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.text.request_layout(id, inspector, window, cx)
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .prepaint(id, inspector, bounds, &mut (), window, cx);
        self.layouts
            .0
            .borrow_mut()
            .insert(self.row, (self.text.layout().clone(), bounds));
    }
    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.text
            .paint(id, inspector, bounds, &mut (), &mut (), window, cx);
    }
}
