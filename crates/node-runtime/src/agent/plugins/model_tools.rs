//! Package declarations select shared provider capabilities, not a second tool runner.
use super::*;
use sailry_protocol::plugin::model_tools::Capability;

pub(super) fn names(capability: Capability) -> &'static [&'static str] {
    match capability {
        Capability::WebSearch => &["openai_web_search", "web_search", "google_search"],
    }
}

pub(super) fn bind(_: &Ingress, _: &Invocation) -> Result<Vec<ToolRegistration>, Fault> {
    Ok(Vec::new())
}

pub(super) fn preferred(_: &Ingress, _: &Invocation) -> bool {
    false
}
