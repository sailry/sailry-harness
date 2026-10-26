//! Compatibility comes from the execution Node; update actions target that owner.
use super::*;
use crate::settings::providers::Binding;
use sailry_protocol::{Fault, FaultDetails};

pub(in crate::settings) fn required(error: &Fault) -> Option<&str> {
    match error.details.as_ref()? {
        FaultDetails::PluginRequiresNewer { required } => Some(required),
        _ => None,
    }
}

fn local(binding: &Binding, cx: &App) -> bool {
    cx.try_global::<crate::backend::Services>()
        .is_some_and(|services| services.local.target() == binding.client.target())
}

pub(in crate::settings) fn message(required: &str, binding: &Binding, cx: &App) -> SharedString {
    if local(binding, cx) {
        rust_i18n::t!("plugins_requires_sailry", version = required).into()
    } else {
        rust_i18n::t!(
            "plugins_requires_node",
            version = required,
            node = binding.label
        )
        .into()
    }
}

pub(in crate::settings) fn error(
    key: &'static str,
    required: Option<&str>,
    binding: &Binding,
    cx: &App,
) -> SharedString {
    required
        .filter(|_| key == "plugins_requires_sailry")
        .map(|required| message(required, binding, cx))
        .unwrap_or_else(|| tr(key))
}

pub(in crate::settings) fn action(
    id: impl Into<ElementId>,
    binding: &Binding,
    cx: &App,
) -> Option<Button> {
    local(binding, cx).then(|| {
        Button::new(id)
            .small()
            .label(tr("plugins_update_sailry"))
            .on_click(|_, window, cx| {
                window.dispatch_action(Box::new(crate::settings::OpenSection(Section::About)), cx);
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn uses_structured_requirement() {
        let mut error = Fault::invalid("requires a newer version");
        assert_eq!(required(&error), None);
        error.details = Some(FaultDetails::PluginRequiresNewer {
            required: "0.1.0-alpha.5".into(),
        });
        assert_eq!(required(&error), Some("0.1.0-alpha.5"));
    }
}
