use super::*;
use core::prelude::v1::test;

#[test]
fn fade_starts_after_half() {
    assert_eq!(fade(0.), 0.);
    assert_eq!(fade(0.5), 0.);
    assert_eq!(fade(0.75), 0.5);
    assert_eq!(fade(1.), 1.);
}
