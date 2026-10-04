//! ADK retains streamed deltas as consecutive parts. Provider serializers may
//! separate parts with newlines, so join model fragments before serialization.
//! Persisted history and user-authored whitespace are left untouched.
use adk_core::{Content, Part};

pub(in crate::agent) fn coalesce(contents: &mut [Content]) {
    for content in contents {
        if !matches!(content.role.as_str(), "model" | "assistant") {
            continue;
        }
        let mut parts = Vec::with_capacity(content.parts.len());
        for part in std::mem::take(&mut content.parts) {
            match (parts.last_mut(), &part) {
                (Some(Part::Text { text: previous }), Part::Text { text }) => {
                    previous.push_str(text)
                }
                (
                    Some(Part::Thinking {
                        thinking: previous,
                        signature: None,
                    }),
                    Part::Thinking {
                        thinking,
                        signature: None,
                    },
                ) => previous.push_str(thinking),
                _ => parts.push(part),
            }
        }
        content.parts = parts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_delta_whitespace() {
        let source = "中文 paragraph\n\n`https://www.example.com`\n\n- next";
        let mut contents = vec![Content {
            role: "model".into(),
            parts: source
                .chars()
                .map(|ch| Part::Text {
                    text: ch.to_string(),
                })
                .collect(),
        }];
        coalesce(&mut contents);
        assert_eq!(
            contents[0].parts,
            vec![Part::Text {
                text: source.into()
            }]
        );
        coalesce(&mut contents);
        assert_eq!(contents[0].parts.len(), 1);
    }

    #[test]
    fn preserves_signed_thought_boundaries() {
        let mut contents = vec![
            Content::new("user").with_text("First").with_text("Second"),
            Content {
                role: "model".into(),
                parts: vec![
                    Part::Thinking {
                        thinking: "a".into(),
                        signature: None,
                    },
                    Part::Thinking {
                        thinking: " b".into(),
                        signature: None,
                    },
                    Part::Thinking {
                        thinking: "signed".into(),
                        signature: Some("opaque".into()),
                    },
                    Part::Text {
                        text: "answer".into(),
                    },
                ],
            },
        ];
        coalesce(&mut contents);
        assert_eq!(contents[0].parts.len(), 2);
        assert_eq!(contents[1].parts.len(), 3);
        assert!(
            matches!(&contents[1].parts[0], Part::Thinking { thinking, signature: None } if thinking == "a b")
        );
        assert!(
            matches!(&contents[1].parts[1], Part::Thinking { signature: Some(value), .. } if value == "opaque")
        );
    }
}
