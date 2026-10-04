use super::{Model, Reasoning};
use crate::{Effort, conversation::ModelApi};

impl Model {
    pub fn efforts(&self, api: ModelApi, output: u32) -> Vec<Effort> {
        let mut choices = Vec::new();
        for option in &self.options {
            let values = match option {
                Reasoning::Effort { values } => values
                    .iter()
                    .filter_map(|value| match value {
                        None => Some(Effort::Default),
                        Some(value) => {
                            serde_json::from_value(serde_json::Value::String(value.clone())).ok()
                        }
                    })
                    .collect(),
                Reasoning::Toggle => vec![],
                Reasoning::BudgetTokens { min, max } => min
                    .iter()
                    .filter_map(|tokens| {
                        if max.is_some_and(|max| *tokens > 0 && *tokens as u32 > max) {
                            return None;
                        }
                        Some(if *tokens == 0 {
                            Effort::Disabled
                        } else {
                            Effort::Budget(*tokens)
                        })
                    })
                    .collect(),
            };
            for value in values {
                if value != Effort::Default
                    && value.validate(api, output).is_ok()
                    && !choices.contains(&value)
                    && choices.len() < 16
                {
                    choices.push(value);
                }
            }
        }
        // An explicit level remains the fallback default; exposing a toggle must
        // not silently turn reasoning off when no enabled level is known.
        if !choices.is_empty()
            && self
                .options
                .iter()
                .any(|option| matches!(option, Reasoning::Toggle))
            && Effort::Disabled.validate(api, output).is_ok()
            && !choices.contains(&Effort::Disabled)
            && choices.len() < 16
        {
            choices.push(Effort::Disabled);
        }
        choices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_preserves_level_order() {
        let mut model = Model {
            id: "fixture".into(),
            name: "Fixture".into(),
            context: None,
            output: Some(8192),
            inputs: vec![],
            outputs: vec![],
            tools: None,
            reasoning: Some(true),
            options: vec![
                Reasoning::Toggle,
                Reasoning::Effort {
                    values: vec![Some("low".into()), Some("high".into()), Some("max".into())],
                },
            ],
        };
        assert_eq!(
            model.efforts(ModelApi::DeepSeek, 8192),
            vec![Effort::Low, Effort::High, Effort::Max, Effort::Disabled]
        );
        model.options.remove(0);
        assert_eq!(
            model.efforts(ModelApi::DeepSeek, 8192),
            vec![Effort::Low, Effort::High, Effort::Max]
        );
    }
}
