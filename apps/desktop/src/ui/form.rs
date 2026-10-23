//! Kit 7fc6a4c exposes label spacing through Form's field size only.
//! Large fields increase the vertical label gap from 2px to 4px without sizing
//! child controls or their text. Keep the ordinary 8px row and 24px column gaps.
use gpui_kit::{
    Styled as _,
    component::{Sizable as _, form::Form},
    px,
};

pub(crate) fn vertical() -> Form {
    Form::vertical().large().gap_y(px(8.)).gap_x(px(24.))
}
