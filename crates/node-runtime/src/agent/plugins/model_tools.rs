//! Saved capabilities remain readable; GDK exposes ordinary search tools only.
use sailry_protocol::plugin::model_tools::Capability;

pub(super) fn names(capability: Capability) -> &'static [&'static str] {
    match capability {
        Capability::WebSearch => &["openai_web_search", "web_search", "google_search"],
    }
}

pub(super) fn preferred(_: &crate::store::Ingress, _: &crate::store::agent::Invocation) -> bool {
    false
}
