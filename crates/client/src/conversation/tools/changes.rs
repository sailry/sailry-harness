//! Detail identity and visible metadata determine whether a tool needs hydration.
use super::Call;
use crate::Client;
use sailry_protocol::{Fault, conversation::Page, tool::detail::Reference};

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

    pub fn retained_bytes(&self) -> usize {
        let metadata = serde_json::to_value(&self.call).expect("serializable tool metadata");
        2 * super::super::details::retained_bytes(&metadata)
            + self.reference.entry.capacity()
            + self.reference.revision.capacity()
            + self.reference.call.as_ref().map_or(0, String::capacity)
    }
}

impl Call {
    pub fn stamp(&self, page: &Page) -> Option<Stamp> {
        Some(Stamp::new(self, self.detail_reference(page)?))
    }
}

impl Client {
    /// Failed reads leave the caller's previous stamp unchanged, allowing a later retry.
    pub async fn changed_tool_call(
        &self,
        call: &Call,
        page: &Page,
        previous: Option<&Stamp>,
    ) -> Result<Option<(Call, Stamp)>, Fault> {
        let stamp = call
            .stamp(page)
            .ok_or_else(|| Fault::invalid("tool detail identity is unavailable"))?;
        if previous == Some(&stamp) {
            return Ok(None);
        }
        let detail = self.tool_detail(stamp.reference.clone()).await?;
        Ok(Some((call.with_detail(detail), stamp)))
    }
}
