//! Project provenance-bound ADK history for one immutable model authorization.
//! Persisted events and user-authored whitespace remain unchanged.
use adk_core::{
    Content, Llm, LlmRequest, LlmResponse, LlmResponseStream, Part, ProviderOrigin, SchemaAdapter,
};
use async_trait::async_trait;
use futures::StreamExt;
use sailry_protocol::{Credential, conversation::Provider};
use sha2::{Digest, Sha256};
use std::sync::Arc;

mod mcp;
pub(in crate::agent) mod output;

struct Model {
    inner: Arc<dyn Llm>,
    origin: ProviderOrigin,
    window: Option<Window>,
}

#[derive(Clone, Copy)]
pub(in crate::agent) struct Window {
    pub(in crate::agent) context: u64,
    pub(in crate::agent) output: u64,
}

impl Window {
    pub(in crate::agent) fn new(provider: &Provider, model: &str) -> Option<Self> {
        let model = provider.models.iter().find(|entry| entry.id == model)?;
        (model.context > 0).then_some(Self {
            context: u64::from(model.context),
            output: u64::from(model.output),
        })
    }
}

pub(super) fn origin(
    provider: &Provider,
    model: &str,
    credential: Option<&Credential>,
    grant: Option<&crate::providers::login::Grant>,
) -> ProviderOrigin {
    use crate::providers::login::Grant;
    // Refresh can preserve replay ownership only with a verified, stable account
    // identity. Other grant kinds conservatively change scope at each revision.
    let owner = match grant {
        Some(Grant::ChatGpt {
            client_id,
            subject,
            account_id,
            ..
        }) => {
            serde_json::json!({"client":client_id,"subject":subject,"account":account_id})
        }
        Some(Grant::ChatGptOAuth {
            subject,
            account_id,
            ..
        }) => {
            serde_json::json!({"subject":subject,"account":account_id})
        }
        _ => serde_json::json!({"revision":credential.map(|credential| credential.revision)}),
    };
    let identity = serde_json::json!({
        "provider":provider.id,
        "authentication":provider.authentication,
        "binding":provider.binding.as_ref().map(|binding| serde_json::json!({
            "package":binding.package.as_ref().map(|package|&package.name),
            "flow":binding.auth,
            "key_injection":binding.key_injection
        })),
        "endpoint":provider.endpoint,
        "credential":credential.map(|credential|credential.id),
        "owner":owner
    });
    ProviderOrigin {
        provider: serde_json::to_value(provider.api)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned(),
        model: model.to_owned(),
        scope: format!("{:x}", Sha256::digest(identity.to_string().as_bytes())),
    }
}

pub(super) fn wrap(
    inner: Arc<dyn Llm>,
    origin: ProviderOrigin,
    window: Option<Window>,
) -> Arc<dyn Llm> {
    Arc::new(Model {
        inner,
        origin,
        window,
    })
}

/// Projects history for one request owner and stamps that owner on its responses.
///
/// Every native path, API-key, OAuth and plugin, generates through this boundary.
pub(super) async fn generate<F, Fut>(
    mut request: LlmRequest,
    origin: ProviderOrigin,
    window: Option<Window>,
    generate: F,
) -> adk_core::Result<LlmResponseStream>
where
    F: FnOnce(LlmRequest) -> Fut,
    Fut: std::future::Future<Output = adk_core::Result<LlmResponseStream>>,
{
    let revisions = output::references(&request.contents);
    project(&mut request, &origin);
    limit(&mut request, window, &revisions)?;
    Ok(responses(generate(request).await?, origin))
}

fn project(request: &mut LlmRequest, origin: &ProviderOrigin) {
    adk_core::history::for_provider(&mut request.contents, origin);
    mcp::project(&mut request.contents);
    coalesce(&mut request.contents);
}

pub(in crate::agent) use output::{Masks, Revisions, masked, remask, revisions};

/// Returns a fingerprint of results shortened for the window, if any.
pub(in crate::agent) fn limit(
    request: &mut LlmRequest,
    window: Option<Window>,
    revisions: &Revisions,
) -> adk_core::Result<Option<String>> {
    output::project_with(&mut request.contents, revisions);
    output::limit(request, window, revisions)
}

pub(in crate::agent) fn mask(
    request: &mut LlmRequest,
    protect: u64,
    minimum: u64,
    revisions: &Revisions,
) -> adk_core::Result<u64> {
    output::mask(request, protect, minimum, revisions)
}

pub(in crate::agent) fn protected(name: &str, value: &serde_json::Value) -> bool {
    output::protected(name, value)
}

fn stamp(response: &mut LlmResponse, origin: &ProviderOrigin) {
    let metadata = response
        .provider_metadata
        .get_or_insert_with(|| serde_json::json!({}));
    if let Some(metadata) = metadata.as_object_mut() {
        metadata.insert(
            "provider_origin".into(),
            serde_json::to_value(origin).unwrap(),
        );
    }
}

fn responses(stream: LlmResponseStream, origin: ProviderOrigin) -> LlmResponseStream {
    Box::pin(stream.map(move |response| {
        response.map(|mut response| {
            stamp(&mut response, &origin);
            response
        })
    }))
}

#[async_trait]
impl Llm for Model {
    fn name(&self) -> &str {
        self.inner.name()
    }
    fn schema_adapter(&self) -> &dyn SchemaAdapter {
        self.inner.schema_adapter()
    }
    fn uses_interactions_api(&self) -> bool {
        self.inner.uses_interactions_api()
    }
    fn max_cost_micro_usd(&self, request: &LlmRequest) -> Option<u64> {
        self.inner.max_cost_micro_usd(request)
    }

    async fn generate_content(
        &self,
        request: LlmRequest,
        stream: bool,
    ) -> adk_core::Result<LlmResponseStream> {
        generate(request, self.origin.clone(), self.window, |request| {
            self.inner.generate_content(request, stream)
        })
        .await
    }
}

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

    fn provider() -> Provider {
        Provider {
            request_max_retries: None,
            stream_max_retries: None,
            id: sailry_protocol::ProviderId::new(),
            revision: 1,
            preset: None,
            binding: None,
            consent: None,
            name: "Fixture".into(),
            api: sailry_protocol::conversation::ModelApi::Responses,
            authentication: sailry_protocol::Authentication::ApiKey,
            endpoint: "https://fixture.test/private-path?key=private-url-token".into(),
            options: None,
            oauth: None,
            enabled: true,
            models: vec![],
            default_model: String::new(),
            credential: None,
        }
    }

    fn credential(provider: &Provider) -> Credential {
        Credential {
            id: sailry_protocol::CredentialId::new(),
            provider: provider.id,
            authentication: provider.authentication,
            account_label: None,
            revision: 1,
            expires_at_ms: None,
            revoked: false,
        }
    }

    #[test]
    fn overwrites_untrusted_response_origins() {
        let provider = provider();
        let expected = origin(&provider, "model", None, None);
        let mut response = LlmResponse {
            provider_metadata: Some(serde_json::json!({
                "provider_origin":{"provider":"untrusted","model":"other","scope":"forged"},
                "conversation_history_parts":[{"thinking":"visible","signature":"opaque"}]
            })),
            ..Default::default()
        };
        stamp(&mut response, &expected);
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["provider_origin"],
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(
            response.provider_metadata.as_ref().unwrap()["conversation_history_parts"][0]["signature"],
            "opaque"
        );
    }

    #[test]
    fn distinguishes_private_authorization_owners() {
        let mut provider = provider();
        let mut credential = credential(&provider);
        let first = origin(&provider, "model", Some(&credential), None);
        assert_eq!(first, origin(&provider, "model", Some(&credential), None));
        credential.revision += 1;
        assert_ne!(first, origin(&provider, "model", Some(&credential), None));
        credential.revision = 1;
        credential.id = sailry_protocol::CredentialId::new();
        assert_ne!(first, origin(&provider, "model", Some(&credential), None));
        provider.endpoint = "https://other.test".into();
        assert_ne!(first, origin(&provider, "model", Some(&credential), None));
        provider.endpoint = "https://other.test/?account=private/".into();
        let endpoint = origin(&provider, "model", Some(&credential), None);
        provider.endpoint.pop();
        assert_ne!(
            endpoint,
            origin(&provider, "model", Some(&credential), None)
        );
        let public = serde_json::to_string(&first).unwrap();
        for secret in ["private-path", "private-url-token", "fixture.test"] {
            assert!(!public.contains(secret));
        }
        assert_eq!(first.scope.len(), 64);
    }

    #[test]
    fn retains_only_verified_refresh_identity() {
        use crate::providers::login::Grant;
        use sailry_protocol::{Authentication, Secret};
        let mut provider = provider();
        provider.authentication = Authentication::ChatGptOAuth;
        let mut credential = credential(&provider);
        let mut grant = Grant::ChatGptOAuth {
            access: Secret::new("private-access".into()),
            refresh: Secret::new("private-refresh".into()),
            expires_at_ms: u64::MAX,
            subject: "private-subject".into(),
            account_id: "private-account".into(),
            email: Some("display@example.test".into()),
            id_token: Secret::new("private-id-token".into()),
            renewal_pending: false,
        };
        let first = origin(&provider, "model", Some(&credential), Some(&grant));
        credential.revision += 1;
        if let Grant::ChatGptOAuth { access, email, .. } = &mut grant {
            *access = Secret::new("rotated-private-access".into());
            *email = Some("changed-display@example.test".into());
        }
        assert_eq!(
            first,
            origin(&provider, "model", Some(&credential), Some(&grant))
        );
        if let Grant::ChatGptOAuth { account_id, .. } = &mut grant {
            *account_id = "other-private-account".into();
        }
        assert_ne!(
            first,
            origin(&provider, "model", Some(&credential), Some(&grant))
        );
        let public = serde_json::to_string(&first).unwrap();
        for private in [
            "private-subject",
            "private-account",
            "private-access",
            "display@",
            "private-url-token",
        ] {
            assert!(!public.contains(private));
        }
        let unverified = Grant::ClaudeOAuth {
            access: Secret::new("access".into()),
            refresh: Secret::new("refresh".into()),
            expires_at_ms: u64::MAX,
            renewal_pending: false,
        };
        let first = origin(&provider, "model", Some(&credential), Some(&unverified));
        credential.revision += 1;
        assert_ne!(
            first,
            origin(&provider, "model", Some(&credential), Some(&unverified))
        );
    }

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

    #[tokio::test]
    async fn references_canonical_mcp_result() {
        let data = serde_json::json!({"text":"canonical evidence 中文🙂".repeat(8_000)});
        let canonical = serde_json::json!({"output":data,"text":[data.to_string()],"isError":true});
        let request = LlmRequest::new(
            "fixture",
            vec![Content {
                role: "function".into(),
                parts: vec![Part::FunctionResponse {
                    id: Some("mcp-call".into()),
                    function_response: adk_core::FunctionResponseData::new(
                        "mcp_fixture",
                        canonical.clone(),
                    ),
                    annotations: None,
                }],
            }],
        );
        let expected = blake3::hash(canonical.to_string().as_bytes())
            .to_hex()
            .to_string();
        let stream = generate(
            request,
            origin(&provider(), "fixture", None, None),
            None,
            |request| async move {
                let Part::FunctionResponse {
                    function_response, ..
                } = &request.contents[0].parts[0]
                else {
                    panic!("tool result expected")
                };
                assert_eq!(function_response.response["output"]["call"], "mcp-call");
                assert_eq!(function_response.response["output"]["revision"], expected);
                assert_eq!(function_response.response["isError"], true);
                Ok(Box::pin(futures::stream::empty()) as LlmResponseStream)
            },
        )
        .await
        .unwrap();
        drop(stream);
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
