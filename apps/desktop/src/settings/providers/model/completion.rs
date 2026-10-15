use super::*;
use sailry_protocol::conversation::{catalog, discovery};

// Only unreported limits in an unsaved draft are eligible for completion.
// Saved limits and explicit user choices never become missing again.
#[derive(Default)]
pub(super) struct Missing {
    pub(super) context: bool,
    output: bool,
}

impl Missing {
    pub fn new(value: &discovery::Model, reference: Option<&catalog::Model>) -> Self {
        Self {
            context: value
                .context
                .or(reference.and_then(|model| model.context))
                .is_none(),
            output: value
                .output
                .or(reference.and_then(|model| model.output))
                .is_none(),
        }
    }
}

impl Draft {
    pub fn fill(
        &mut self,
        native: Option<&discovery::Model>,
        reference: Option<&catalog::Model>,
        preset: super::super::Preset,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Ok(current) = self.value(cx) else {
            return;
        };
        self.missing.context &= current.context == self.model.context;
        let capabilities = native
            .map(|value| value.capabilities.clone())
            .unwrap_or_else(|| current.capabilities.clone());
        // Fill absent choices without replacing configured lists or defaults.
        let refresh_efforts = !current.custom_efforts
            && current
                .efforts
                .iter()
                .all(|effort| *effort == Effort::Default);
        let completed = Self::discovered(
            self.key,
            discovery::Model {
                id: current.id.clone(),
                display_name: native
                    .and_then(|model| model.display_name.clone())
                    .or(current.display_name.clone()),
                context: if self.missing.context {
                    native.and_then(|value| value.context)
                } else {
                    Some(current.context)
                },
                output: if self.missing.output {
                    native.and_then(|value| value.output)
                } else {
                    Some(current.output)
                },
                capabilities,
            },
            reference,
            preset,
            window,
            cx,
        );
        let mut model = completed.model;
        model.custom_efforts = current.custom_efforts;
        if model
            .capabilities
            .as_ref()
            .and_then(|value| value.reasoning)
            .or(reference.and_then(|value| value.reasoning))
            .is_none()
        {
            model.reasoning = model
                .capabilities
                .as_ref()
                .and_then(|value| value.efforts.as_ref())
                .is_some_and(|efforts| efforts.iter().any(|effort| *effort != Effort::Default))
                || current
                    .efforts
                    .iter()
                    .any(|effort| *effort != Effort::Default);
        }
        model.web_search = model
            .capabilities
            .as_ref()
            .and_then(|value| value.web_search)
            .unwrap_or(current.web_search);
        if !refresh_efforts || model.efforts.is_empty() {
            model.efforts = current.efforts;
            model.default_effort = current.default_effort;
        }
        if self.missing.context && model.context != current.context {
            self.context.update(cx, |input, cx| {
                input.set_value(model.context.to_string(), window, cx)
            });
        }
        self.missing = completed.missing;
        if !current.custom_efforts {
            self.efforts = reasoning::Rows::new(&model, window, cx);
        }
        self.model = model;
    }
}
