use super::*;
use core::prelude::v1::test;
fn entry(name: &str, path: &str) -> protocol::Candidate {
    protocol::Candidate {
        name: name.into(),
        path: path.into(),
        skill: sailry_protocol::plugin::Skill {
            name: name.into(),
            path: path.into(),
            description: String::new(),
            display_name: None,
            display_locales: Default::default(),
            license: None,
            compatibility: None,
        },
    }
}
#[test]
fn selects_actual_documents() {
    let entries = [
        entry("design", "skills/design"),
        entry("review", "other/review"),
    ];
    assert_eq!(candidate(&entries, "review").unwrap().path, "other/review");
    assert!(candidate(&entries, "unknown").is_none());
    let ambiguous = [
        entry("review", "first/review"),
        entry("review", "second/review"),
    ];
    assert!(candidate(&ambiguous, "review").is_none());
    assert_eq!(
        candidate(&entries[..1], "directory-id").unwrap().path,
        "skills/design"
    );
}

struct Documents {
    node: sailry_protocol::NodeId,
    arrived: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}

impl sailry_link::Transport for Documents {
    fn target(&self) -> sailry_protocol::NodeId {
        self.node
    }

    fn dispatch(
        &self,
        request: sailry_protocol::Request,
    ) -> sailry_link::Pending<'_, Result<sailry_link::Admission, Fault>> {
        Box::pin(async move {
            let Command::ReadSkillDocument {
                source: protocol::DocumentSource::Bundled { skill, .. },
            } = request.command
            else {
                panic!("skill document request expected");
            };
            if skill == "old" {
                self.arrived.add_permits(1);
                self.release.acquire().await.unwrap().forget();
            }
            if skill == "invalid" {
                return Err(Fault::invalid("invalid skill source"));
            }
            let output = if skill == "wrong" {
                Output::Plugins(Vec::new())
            } else {
                Output::SkillDocument(protocol::Document {
                    skill: entry(&skill, "SKILL.md").skill,
                    content: skill,
                })
            };
            let (send, completion) = tokio::sync::oneshot::channel();
            send.send(Ok(output)).unwrap();
            Ok(sailry_link::Admission {
                receipt: sailry_protocol::Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            })
        })
    }

    fn subscribe(
        &self,
        _: sailry_protocol::Topic,
    ) -> sailry_link::Pending<'_, Result<Box<dyn sailry_link::Subscription>, Fault>> {
        Box::pin(async { Err(Fault::invalid("unexpected subscription")) })
    }
}

fn listing(name: &str) -> skillcatalog::Entry {
    skillcatalog::Entry {
        id: name.into(),
        package: Some("fixture".into()),
        name: name.into(),
        display_name: None,
        icon: None,
        display_locales: Default::default(),
        description: None,
        description_locales: Default::default(),
        origin: "fixture".into(),
        url: None,
        source: None,
        bundled: true,
    }
}

#[test]
fn discards_replaced_reads_and_preserves_failures() {
    let runtime = std::sync::Arc::new(tokio::runtime::Runtime::new().unwrap());
    let transport = std::sync::Arc::new(Documents {
        node: sailry_protocol::NodeId([1; 32]),
        arrived: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Semaphore::new(0),
    });
    let binding = Binding {
        client: std::sync::Arc::new(sailry_client::Client::new(transport.clone())),
        runtime: runtime.clone(),
        label: "Fixture".into(),
    };
    runtime.block_on(async {
        let old = CancellationToken::new();
        let task = tokio::spawn({
            let binding = binding.clone();
            let stop = old.child_token();
            async move { read(&binding, &listing("old"), &stop).await }
        });
        transport.arrived.acquire().await.unwrap().forget();
        old.cancel();
        let current = CancellationToken::new();
        let (_, document) = read(&binding, &listing("latest"), &current)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(document.unwrap().content, "latest");
        transport.release.add_permits(1);
        assert!(task.await.unwrap().is_none());
        let wrong = read(&binding, &listing("wrong"), &current)
            .await
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(wrong.code, sailry_protocol::ErrorCode::Internal);
        let invalid = read(&binding, &listing("invalid"), &current)
            .await
            .unwrap()
            .err()
            .unwrap();
        assert_eq!(invalid.code, sailry_protocol::ErrorCode::InvalidRequest);
        current.cancel();
        assert!(read(&binding, &listing("latest"), &current).await.is_none());
    });
}
