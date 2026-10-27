//! Detail identity and visible metadata determine whether a tool needs hydration.
use super::Call;
use sailry_protocol::{conversation::Page, tool::detail::Reference};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub reference: Reference,
    call: Call,
}

impl Stamp {
    pub fn new(call: &Call, reference: Reference) -> Self {
        let mut call = call.clone();
        call.detail = None;
        Self { reference, call }
    }
}

impl Call {
    pub fn stamp(&self, page: &Page) -> Option<Stamp> {
        Some(Stamp::new(self, self.detail_reference(page)?))
    }
}
