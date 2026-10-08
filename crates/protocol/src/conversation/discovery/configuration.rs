use super::{Model, ModelApi};
use crate::{Authentication, Effort, conversation::catalog, media::Generation};

const DEFAULT_CONTEXT: u32 = 200_000;
const DEFAULT_OUTPUT: u32 = 16_384;

impl Model {
    /// Resolve reported limits and automatic capabilities against an optional reference.
    /// Callers retain configured limits and explicit reasoning choices when refreshing.
    pub fn configuration(
        &self,
        reference: Option<&catalog::Model>,
        api: ModelApi,
        _authentication: Authentication,
    ) -> crate::conversation::Model {
        let reported_output = self.output.or(reference.and_then(|model| model.output));
        let context = self
            .context
            .or(reference.and_then(|model| model.context))
            .unwrap_or_else(|| DEFAULT_CONTEXT.max(reported_output.unwrap_or(0)));
        let output = reported_output.unwrap_or(DEFAULT_OUTPUT.min(context));
        let capabilities = self.capabilities.as_ref();
        let mut efforts = capabilities
            .and_then(|value| value.efforts.clone())
            .unwrap_or_else(|| {
                reference
                    .map(|model| model.efforts(api, output))
                    .unwrap_or_default()
            });
        efforts.retain(|effort| *effort != Effort::Default);
        let default_effort = Effort::initial(
            &efforts,
            capabilities
                .and_then(|value| value.default_effort)
                .unwrap_or(Effort::Default),
        );
        crate::conversation::Model {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            context,
            output,
            capabilities: self.capabilities.clone(),
            vision: capabilities
                .and_then(|value| value.vision)
                .unwrap_or_else(|| {
                    reference
                        .filter(|model| !model.inputs.is_empty())
                        .is_none_or(|model| model.inputs.iter().any(|value| value == "image"))
                }),
            tools: capabilities
                .and_then(|value| value.tools)
                .or(reference.and_then(|model| model.tools))
                // Missing metadata is not a denial. The selected API receives
                // ordinary tools and reports unsupported features normally.
                .unwrap_or(true),
            reasoning: capabilities
                .and_then(|value| value.reasoning)
                .or(reference.and_then(|model| model.reasoning))
                .unwrap_or(false),
            web_search: capabilities
                .and_then(|value| value.web_search)
                .unwrap_or(false),
            generates: reference
                .map(|model| {
                    model
                        .outputs
                        .iter()
                        .filter_map(|output| match output.as_str() {
                            "image" => Some(Generation::Image),
                            "video" => Some(Generation::Video),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            efforts,
            custom_efforts: false,
            default_effort,
        }
    }
}

#[cfg(test)]
mod tests;
