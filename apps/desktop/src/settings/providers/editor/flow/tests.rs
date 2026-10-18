use super::*;
use core::prelude::v1::test;

#[test]
fn skips_single_method() {
    let mut flow = Flow::new(false);
    flow.select("single", 1);
    assert!(flow.step == Step::Configure);
    flow.back(1);
    assert!(flow.step == Step::Brand);
}

#[test]
fn returns_through_method_selection() {
    let mut flow = Flow::new(false);
    flow.select("multiple", 3);
    assert!(flow.step == Step::Method);
    flow.step = Step::Configure;
    flow.back(3);
    assert!(flow.step == Step::Method);
    flow.back(3);
    assert!(flow.step == Step::Brand);
}

#[test]
fn editing_cannot_change_type() {
    let mut flow = Flow::new(true);
    flow.select("other", 3);
    flow.back(3);
    assert!(flow.step == Step::Configure);
    assert!(flow.brand.is_none());
}
