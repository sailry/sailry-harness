use gpui_kit::{component::scroll::ScrollbarHandle, *};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, Default)]
pub(super) struct Metrics {
    pub bounds: Bounds<Pixels>,
    pub cell: Size<Pixels>,
    pub columns: u16,
    pub rows: u16,
}

#[derive(Clone, Default)]
pub(super) struct Scroll(Rc<Cell<Geometry>>);

#[derive(Clone, Copy, Default)]
struct Geometry {
    bounds: Bounds<Pixels>,
    content: Size<Pixels>,
    offset: Point<Pixels>,
    bottom: bool,
}

impl Scroll {
    pub fn at_bottom(&self) -> bool {
        self.0.get().bottom
    }
    pub fn layout(&self, bounds: Bounds<Pixels>, content: Size<Pixels>) {
        let mut geometry = self.0.get();
        let bottom = geometry.bottom
            || geometry.offset.y
                <= (geometry.bounds.size.height - geometry.content.height).min(px(0.)) + px(1.);
        geometry.bounds = bounds;
        geometry.content = content;
        let minimum = (bounds.size.height - content.height).min(px(0.));
        geometry.offset.y = if bottom {
            minimum
        } else {
            geometry.offset.y.max(minimum).min(px(0.))
        };
        geometry.bottom = bottom;
        self.0.set(geometry);
    }

    pub fn bottom(&self) {
        let mut geometry = self.0.get();
        geometry.bottom = true;
        geometry.offset.y = (geometry.bounds.size.height - geometry.content.height).min(px(0.));
        self.0.set(geometry);
    }

    pub fn move_by(&self, delta: Pixels) {
        let mut offset = self.offset();
        offset.y += delta;
        self.set_offset(offset);
    }
}

impl ScrollbarHandle for Scroll {
    fn viewport_bounds(&self) -> Bounds<Pixels> {
        self.0.get().bounds
    }
    fn content_size(&self) -> Size<Pixels> {
        self.0.get().content
    }
    fn offset(&self) -> Point<Pixels> {
        self.0.get().offset
    }
    fn set_offset(&self, offset: Point<Pixels>) {
        let mut geometry = self.0.get();
        let minimum = (geometry.bounds.size.height - geometry.content.height).min(px(0.));
        geometry.offset.y = offset.y.max(minimum).min(px(0.));
        geometry.bottom = geometry.offset.y <= minimum + px(1.);
        self.0.set(geometry);
    }
}
