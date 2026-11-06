use super::{Mode, NAME, Options, transfer};
use sailry_client::Client;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{
    Authentication, Command, CredentialId, CredentialRef, ErrorCode, Fault, NodeId, Output,
    ProviderId, Receipt, Request, RequestId, RequestOutcome, Secret, Topic,
    conversation::{Model, ModelApi, Provider, discovery},
};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy)]
enum Behavior {
    Complete,
    LostCompletion,
    Unknown,
    IncompleteValidation,
}

struct Stub {
    node: NodeId,
    items: Mutex<Vec<Provider>>,
    calls: Mutex<Vec<&'static str>>,
    original: Mutex<Option<(RequestId, String)>>,
    behavior: Behavior,
}

impl Stub {
    fn client(self: &Arc<Self>) -> Client {
        Client::new(self.clone())
    }

    fn count(&self, command: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| **call == command)
            .count()
    }
}

impl Transport for Stub {
    fn target(&self) -> NodeId {
        self.node
    }

    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            assert!(request.target == self.node);
            let id = request.id;
            let durable = request.command.durable();
            if matches!(request.command, Command::SaveProvider { .. }) {
                let mut hasher = blake3::Hasher::new();
                serde_json::to_writer(&mut hasher, &request).unwrap();
                *self.original.lock().unwrap() = Some((id, hasher.finalize().to_hex().to_string()));
            }
            let (name, response) = match request.command {
                Command::ListProviders => (
                    "list",
                    Some(Ok(Output::Providers(self.items.lock().unwrap().clone()))),
                ),
                Command::ReadProviderKey {
                    provider,
                    expected_revision,
                } => {
                    let items = self.items.lock().unwrap();
                    assert!(
                        items
                            .iter()
                            .any(|item| item.id == provider && item.revision == expected_revision)
                    );
                    (
                        "key",
                        Some(Ok(Output::ProviderKey(Some(Secret::new(
                            "fake-memory-key".into(),
                        ))))),
                    )
                }
                Command::SaveProvider {
                    mut provider,
                    expected_revision,
                    secret,
                } => {
                    assert!(self.node == NodeId([2; 32]));
                    assert!(expected_revision == 0 && provider.revision == 0);
                    assert!(provider.credential.is_none());
                    assert!(secret == Some(Secret::new("fake-memory-key".into())));
                    assert!(
                        !self
                            .items
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|item| item.id == provider.id)
                    );
                    provider.revision = 1;
                    provider.credential = Some(CredentialRef {
                        node: self.node,
                        id: CredentialId::new(),
                    });
                    self.items.lock().unwrap().push(provider.clone());
                    let response = match self.behavior {
                        Behavior::LostCompletion | Behavior::Unknown => None,
                        _ => Some(Ok(Output::Provider(provider))),
                    };
                    ("save", response)
                }
                Command::InspectRequest {
                    id: original,
                    digest,
                } => {
                    let expected = self.original.lock().unwrap();
                    let (expected_id, expected_digest) = expected.as_ref().unwrap();
                    assert!(original == *expected_id && digest == *expected_digest);
                    assert!(id != original);
                    let outcome = match self.behavior {
                        Behavior::LostCompletion => RequestOutcome::Completed(Box::new(Ok(
                            Output::Provider(self.items.lock().unwrap().last().unwrap().clone()),
                        ))),
                        _ => RequestOutcome::Unknown,
                    };
                    (
                        "inspect",
                        Some(Ok(Output::RequestOutcome {
                            id: original,
                            outcome,
                        })),
                    )
                }
                Command::ValidateProvider {
                    provider,
                    expected_revision,
                } => {
                    assert!(
                        self.items
                            .lock()
                            .unwrap()
                            .iter()
                            .any(|item| item.id == provider && item.revision == expected_revision)
                    );
                    (
                        "validate",
                        Some(Ok(Output::ProviderValidation(discovery::Validation {
                            provider,
                            revision: expected_revision,
                            missing: vec![],
                            exceeded: vec![],
                            unverified: if matches!(self.behavior, Behavior::IncompleteValidation) {
                                vec!["test-model".into()]
                            } else {
                                vec![]
                            },
                        }))),
                    )
                }
                _ => {
                    return Err(Fault::new(
                        ErrorCode::InvalidRequest,
                        "unexpected fixture command",
                    ));
                }
            };
            self.calls.lock().unwrap().push(name);
            let (sender, completion) = tokio::sync::oneshot::channel();
            if let Some(response) = response {
                let _ = sender.send(response);
            }
            Ok(Admission {
                receipt: Receipt { id, durable },
                completion,
            })
        })
    }

    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        unreachable!("the helper does not subscribe")
    }
}

fn provider(name: &str) -> Provider {
    Provider {
        id: ProviderId::new(),
        revision: 7,
        preset: None,
        binding: None,
        consent: None,
        name: name.into(),
        api: ModelApi::Responses,
        authentication: Authentication::ApiKey,
        endpoint: "https://provider.invalid/v1".into(),
        options: None,
        oauth: None,
        enabled: true,
        default_model: "test-model".into(),
        request_max_retries: Some(2),
        stream_max_retries: Some(1),
        models: vec![Model {
            id: "test-model".into(),
            display_name: Some("Test model".into()),
            context: 64000,
            output: 4000,
            compaction_tokens: Some(48000),
            capabilities: None,
            vision: true,
            tools: true,
            reasoning: true,
            generates: vec![],
            efforts: vec![],
            custom_efforts: true,
            default_effort: Default::default(),
        }],
        credential: Some(CredentialRef {
            node: NodeId([1; 32]),
            id: CredentialId::new(),
        }),
    }
}

fn fixture(items: Vec<Provider>, behavior: Behavior) -> (Arc<Stub>, Arc<Stub>) {
    let stub = |node, items, behavior| {
        Arc::new(Stub {
            node,
            items: Mutex::new(items),
            calls: Mutex::new(vec![]),
            original: Mutex::new(None),
            behavior,
        })
    };
    (
        stub(NodeId([1; 32]), items, Behavior::Complete),
        stub(NodeId([2; 32]), vec![], behavior),
    )
}

#[tokio::test]
async fn prepare_never_reads_a_key() {
    let (source, target) = fixture(vec![provider(NAME)], Behavior::Complete);
    assert!(
        transfer::prepare(&source.client(), &target.client(), Some(7))
            .await
            .is_ok()
    );
    assert!(source.count("key") == 0 && target.count("save") == 0 && target.count("validate") == 0);
}

#[tokio::test]
async fn rejects_missing_ambiguous_and_non_key_sources() {
    let mut login = provider(NAME);
    login.authentication = Authentication::ChatGptOAuth;
    let mut package = provider(NAME);
    package.api = ModelApi::Plugin;
    for (items, status) in [
        (vec![provider("星桥 ")], "exact_provider_name_not_found"),
        (
            vec![provider(NAME), provider(NAME)],
            "exact_provider_name_is_ambiguous",
        ),
        (vec![login], "only_api_key_authentication_is_supported"),
        (
            vec![package],
            "package_binding_requires_target_owned_configuration_no_copy",
        ),
    ] {
        let (source, target) = fixture(items, Behavior::Complete);
        assert!(
            transfer::prepare(&source.client(), &target.client(), None)
                .await
                .err()
                == Some(status)
        );
        assert!(source.count("key") == 0 && target.count("save") == 0);
    }
}

#[tokio::test]
async fn rejects_existing_target_and_changed_revision() {
    let (source, target) = fixture(vec![provider(NAME)], Behavior::Complete);
    assert!(
        transfer::prepare(&source.client(), &target.client(), Some(6))
            .await
            .err()
            == Some("source_provider_revision_changed")
    );
    target.items.lock().unwrap().push(provider(NAME));
    assert!(
        transfer::prepare(&source.client(), &target.client(), Some(7))
            .await
            .err()
            == Some("target_matching_provider_exists_no_copy")
    );
    assert!(source.count("key") == 0 && target.count("save") == 0);
}

#[tokio::test]
async fn preserves_metadata_and_validates_one_new_provider() {
    let original = provider(NAME);
    let (source, target) = fixture(vec![original.clone()], Behavior::Complete);
    let mut unrelated = provider("Other");
    unrelated.endpoint = "https://other.invalid".into();
    unrelated.credential = None;
    target.items.lock().unwrap().push(unrelated.clone());
    let plan = transfer::prepare(&source.client(), &target.client(), Some(7))
        .await
        .unwrap();
    transfer::copy(&source.client(), &target.client(), plan)
        .await
        .unwrap();
    let items = target.items.lock().unwrap();
    assert!(items.len() == 2 && items[0] == unrelated);
    let saved = &items[1];
    assert!(saved.id != original.id && saved.revision == 1);
    assert!(
        saved
            .credential
            .as_ref()
            .is_some_and(|reference| reference.node == target.node)
    );
    let mut expected = original;
    expected.id = saved.id;
    expected.revision = saved.revision;
    expected.credential = saved.credential.clone();
    assert!(*saved == expected);
    assert!(source.count("key") == 1 && target.count("save") == 1 && target.count("validate") == 1);
}

#[tokio::test]
async fn lost_completion_only_inspects_original_request() {
    let (source, target) = fixture(vec![provider(NAME)], Behavior::LostCompletion);
    let plan = transfer::prepare(&source.client(), &target.client(), Some(7))
        .await
        .unwrap();
    transfer::copy(&source.client(), &target.client(), plan)
        .await
        .unwrap();
    assert!(
        target.count("save") == 1 && target.count("inspect") == 1 && target.count("validate") == 1
    );
}

#[tokio::test]
async fn unknown_outcome_never_replays_or_claims_validation() {
    let (source, target) = fixture(vec![provider(NAME)], Behavior::Unknown);
    let plan = transfer::prepare(&source.client(), &target.client(), Some(7))
        .await
        .unwrap();
    assert!(
        transfer::copy(&source.client(), &target.client(), plan)
            .await
            .err()
            == Some("save_outcome_unknown_no_replay")
    );
    assert!(
        target.count("save") == 1 && target.count("inspect") == 1 && target.count("validate") == 0
    );
    assert!(
        transfer::prepare(&source.client(), &target.client(), Some(7))
            .await
            .err()
            == Some("target_matching_provider_exists_no_copy")
    );
    assert!(source.count("key") == 1 && target.count("save") == 1);
}

#[tokio::test]
async fn incomplete_validation_is_not_success() {
    let (source, target) = fixture(vec![provider(NAME)], Behavior::IncompleteValidation);
    let plan = transfer::prepare(&source.client(), &target.client(), Some(7))
        .await
        .unwrap();
    assert!(
        transfer::copy(&source.client(), &target.client(), plan)
            .await
            .err()
            == Some("provider_validation_incomplete_no_model_generation")
    );
    assert!(target.count("save") == 1 && target.count("validate") == 1);
}

#[test]
fn requires_exact_copy_approval() {
    let base = || {
        vec![
            "--controller-dir".into(),
            std::env::temp_dir()
                .join("sailry-copy-controller")
                .to_string_lossy()
                .into_owned(),
            "--source-node".into(),
            NodeId([1; 32]).to_string(),
            "--target-node".into(),
            NodeId([2; 32]).to_string(),
        ]
    };
    assert!(matches!(
        Options::parse(base()).unwrap().mode,
        Mode::Prepare
    ));
    for (approved, name, revision, validate) in [
        (NodeId([1; 32]), NAME, "7", true),
        (NodeId([2; 32]), "星桥 ", "7", true),
        (NodeId([2; 32]), NAME, "0", true),
        (NodeId([2; 32]), NAME, "7", false),
    ] {
        let mut arguments = base();
        arguments.extend([
            "--copy-to-node".into(),
            approved.to_string(),
            "--provider-name".into(),
            name.into(),
            "--provider-revision".into(),
            revision.into(),
        ]);
        if validate {
            arguments.push("--validate-target".into());
        }
        assert!(Options::parse(arguments).is_err());
    }
    let mut arguments = base();
    arguments.extend([
        "--copy-to-node".into(),
        NodeId([2; 32]).to_string(),
        "--provider-name".into(),
        NAME.into(),
        "--provider-revision".into(),
        "7".into(),
        "--validate-target".into(),
    ]);
    assert!(matches!(
        Options::parse(arguments).unwrap().mode,
        Mode::Copy { revision: 7 }
    ));
}
