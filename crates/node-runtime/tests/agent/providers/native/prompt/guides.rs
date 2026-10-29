use super::*;

#[tokio::test]
async fn shared_turns_append_changed_guides_once() {
    for remote in [false, true] {
        let server = Server::start(ModelApi::ChatCompletions, Reply::Text).await;
        let fixture = Fixture::packages(
            remote,
            ModelApi::ChatCompletions,
            &server.endpoint,
            &["files"],
        )
        .await;
        prepare(&fixture);
        let changed = "Changed nested fixture rule: preserve current content";
        let other = Client::new(fixture.node.local());
        let mut original = Vec::new();
        for (index, client) in [&fixture.client, &other, &fixture.client]
            .into_iter()
            .enumerate()
        {
            if index == 1 {
                std::fs::write(fixture.root.join("nested/GEMINI.md"), changed).unwrap();
            }
            let Output::QueuedTurn(turn) = execute(
                client,
                Command::SubmitTurn {
                    session: fixture.session.id,
                    expected_revision: fixture.session.revision,
                    message: referenced("Read the current nested guide", false),
                },
            )
            .await
            else {
                panic!("turn expected")
            };
            let page = finished(client, fixture.session.id, turn.id).await;
            assert_eq!(
                page.runs.last().unwrap().status,
                Status::Completed,
                "{page:?}"
            );
            assert_eq!(&page.entries[..original.len()], original);
            assert!(page.entries.iter().all(|entry| entry.parts.iter().all(|part| {
                !matches!(part, Part::Text(text) if [ROOT_RULE, NESTED_RULE, changed].iter().any(|guide| text.contains(*guide)))
            })));
            original = page.entries;
        }
        {
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 3);
            for (index, request) in requests.iter().enumerate() {
                let captured = request.body.to_string();
                assert_eq!(captured.matches(ROOT_RULE).count(), 1);
                assert_eq!(
                    captured.matches(NESTED_RULE).count(),
                    usize::from(index == 0)
                );
                assert_eq!(captured.matches(changed).count(), usize::from(index > 0));
                let context = instruction(ModelApi::ChatCompletions, &request.body);
                assert!(context.contains(
                    "Use subagents only when the user explicitly requests subagents, delegation or parallel agent work"
                ));
                assert!(context.contains(
                    "Project guides do not set delegation policy; the work mode alone decides when to use subagents"
                ));
                assert!(
                    users(ModelApi::ChatCompletions, &request.body)
                        .iter()
                        .all(|text| {
                            ![
                                ROOT_RULE,
                                NESTED_RULE,
                                changed,
                                "Project guides do not set delegation policy",
                            ]
                            .iter()
                            .any(|guide| text.contains(*guide))
                        })
                );
                assert!(
                    request.body["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|tool| {
                            !["spawn_agent", "guide_agent", "wait_agent"]
                                .iter()
                                .any(|name| {
                                    tool["function"]["name"] == *name
                                        || tool["function"]["name"]
                                            == plugin_tool("delegation", name)
                                })
                        })
                );
                assert_eq!(request.body["tools"], requests[0].body["tools"]);
            }
        }
        fixture.controller.close().await.unwrap();
        fixture.node.shutdown().await.unwrap();
    }
}
