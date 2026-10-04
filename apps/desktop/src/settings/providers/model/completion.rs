use super::*;
use sailry_protocol::conversation::{catalog, discovery};

// Only unreported fields in an unsaved draft are eligible for completion.
// Saved models and explicit user choices never become missing again.
#[derive(Default)]
pub(super) struct Missing {
    pub(super) context: bool,
    output: bool,
    pub vision: bool,
    pub tools: bool,
    pub reasoning: bool,
    pub generates: bool,
}

impl Missing {
    pub fn new(value: &discovery::Model, reference: Option<&catalog::Model>) -> Self {
        let capabilities = value.capabilities.as_ref();
        Self {
            context: value
                .context
                .or(reference.and_then(|model| model.context))
                .is_none(),
            output: value
                .output
                .or(reference.and_then(|model| model.output))
                .is_none(),
            vision: capabilities.and_then(|value| value.vision).is_none()
                && reference.is_none_or(|model| model.inputs.is_empty()),
            tools: capabilities
                .and_then(|value| value.tools)
                .or(reference.and_then(|model| model.tools))
                .is_none(),
            reasoning: capabilities
                .and_then(|value| value.reasoning)
                .or(reference.and_then(|model| model.reasoning))
                .is_none(),
            generates: reference.is_none_or(|model| model.outputs.is_empty()),
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
        let capabilities = native.and_then(|value| value.capabilities.as_ref());
        // Automatic choices may refresh; explicit edits remain authoritative.
        let refresh_efforts = !current.custom_efforts
            && (capabilities.is_some_and(|value| value.efforts.is_some())
                || reference.is_some_and(|model| !model.options.is_empty()));
        let completed = Self::discovered(
            self.key,
            discovery::Model {
                id: current.id.clone(),
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
                capabilities: Some(discovery::Capabilities {
                    web_search: capabilities
                        .and_then(|value| value.web_search)
                        .or(Some(current.web_search)),
                    vision: if self.missing.vision {
                        capabilities.and_then(|value| value.vision)
                    } else {
                        Some(current.vision)
                    },
                    tools: if self.missing.tools {
                        capabilities.and_then(|value| value.tools)
                    } else {
                        Some(current.tools)
                    },
                    reasoning: if self.missing.reasoning {
                        capabilities.and_then(|value| value.reasoning)
                    } else {
                        Some(current.reasoning)
                    },
                    efforts: if refresh_efforts {
                        capabilities.and_then(|value| value.efforts.clone())
                    } else {
                        Some(current.efforts.clone())
                    },
                    default_effort: if refresh_efforts {
                        capabilities.and_then(|value| value.default_effort)
                    } else {
                        Some(current.default_effort)
                    },
                }),
            },
            reference,
            preset,
            window,
            cx,
        );
        let mut model = completed.model;
        model.custom_efforts = current.custom_efforts;
        if !self.missing.generates {
            model.generates = current.generates;
        }
        if !refresh_efforts && !current.efforts.is_empty() {
            model.efforts = current.efforts;
            model.default_effort = current.default_effort;
        }
        if self.missing.context && model.context != current.context {
            self.context.update(cx, |input, cx| {
                input.set_value(model.context.to_string(), window, cx)
            });
        }
        let generates = self.missing.generates && completed.missing.generates;
        self.missing = completed.missing;
        self.missing.generates = generates;
        if !current.custom_efforts {
            self.efforts = reasoning::Rows::new(&model, window, cx);
        }
        self.model = model;
    }
}

#[cfg(test)]
mod tests;
