use super::*;
use core::prelude::v1::test;

#[test]
fn intersection_visibility() {
    let mask = Bounds::new(point(px(10.), px(10.)), size(px(20.), px(20.)));
    let glyph = |x, y| Bounds::new(point(px(x), px(y)), size(px(16.), px(16.)));
    assert!(visible(glyph(0., 0.), mask));
    assert!(visible(glyph(29., 29.), mask));
    assert!(!visible(glyph(30., 10.), mask));
    assert!(!visible(glyph(-6., 10.), mask));
}
