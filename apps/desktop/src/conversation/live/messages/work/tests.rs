use super::*;
use core::prelude::v1::test;

#[test]
fn keeps_final_prose_outside_work() {
    let blocks = [
        Block::Text("intro".into(), "Before".into()),
        Block::Thinking("reason".into(), "Reason".into()),
        Block::Tools(vec![]),
        Block::Text("progress".into(), "Progress update".into()),
        Block::Tools(vec![]),
        Block::Text("answer".into(), "After".into()),
    ];
    assert_eq!(answer_from(&blocks), 5);
    assert_eq!(answer_from(&blocks[5..]), 0);
}
