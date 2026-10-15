use super::*;
use core::prelude::v1::test;
use sailry_protocol::TurnId;

#[test]
fn preserves_page_overlap() {
    let turns: Vec<_> = (0..6).map(|_| TurnId::new()).collect();
    assert_eq!(insertion(&turns[2..4], &turns), Some((2, 2)));
    assert_eq!(insertion(&[], &turns), Some((0, 6)));
    assert_eq!(insertion(&turns, &turns), Some((0, 0)));
    assert_eq!(insertion(&turns[2..], &turns), Some((2, 0)));
    assert_eq!(insertion(&turns[..4], &turns), Some((0, 2)));
    assert_eq!(insertion(&turns, &turns[1..]), None);
}
