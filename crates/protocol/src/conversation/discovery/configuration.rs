use super::{Model, ModelApi};
use crate::{Effort, conversation::catalog, media::Generation};

const DEFAULT_CONTEXT: u32 = 200_000;
const DEFAULT_OUTPUT: u32 = 16_384;

impl Model {
    /// Complete a new model from native metadata and an optional cached reference.
    /// Saved configuration is never passed through this resolver.
    pub fn configuration(
        &self,
        reference: Option<&catalog::Model>,
        api: ModelApi,
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
            context,
            output,
            vision: capabilities
                .and_then(|value| value.vision)
                .unwrap_or_else(|| {
                    reference.is_some_and(|model| model.inputs.iter().any(|value| value == "image"))
                }),
            tools: capabilities
                .and_then(|value| value.tools)
                .unwrap_or_else(|| reference.is_some_and(|model| model.tools == Some(true))),
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
