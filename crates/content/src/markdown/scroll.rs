//! Use Kit's existing scroll ownership for Bezel's horizontal content viewports.
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::{
    AnyElement, Element, ElementId, InteractiveElement, IntoElement, ParentElement, Styled,
};

pub(super) fn horizontal<E>(id: impl Into<ElementId>, element: E) -> AnyElement
where
    E: InteractiveElement + Styled + ParentElement + Element + ScrollableElement + 'static,
{
    element.overflow_x_scrollbar().id(id).into_any_element()
}
