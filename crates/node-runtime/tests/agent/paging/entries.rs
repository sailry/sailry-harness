use super::*;

#[tokio::test]
async fn streams_the_same_bounded_projection() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let contents = "captured source payload ".repeat(4096);
        std::fs::write(root.join("source.txt"), &contents).unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let controller =
            Link::controller(directory.path().join("controller"), NetworkScope::default())
                .await
                .unwrap();
        let address = controller
            .handle()
            .pair(node.link().invite().unwrap().ticket())
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(address)
        } else {
            node.local()
        });
        let server = Server::tools(vec![(
            plugin_tool("files", "read_file"),
            json!({"path":"source.txt"}),
        )])
        .await;
        let session = approvals::prepare_files(&client, &server, &root).await;
        let mut updates = client.subscribe_conversation(session.id).await.unwrap();
        assert!(matches!(
            updates.next().await.unwrap(),
            Update::ConversationSnapshot(_)
        ));
        let turn = submit(&client, session.id).await;
        let streamed = tokio::time::timeout(Duration::from_secs(10), async {
            let mut streamed = Vec::new();
            loop {
                let Update::ConversationFrame(frame) = updates.next().await.unwrap() else {
                    panic!("conversation frame expected")
                };
                let Change::Entry(entry) = frame.change else {
                    continue;
                };
                if entry.turn != turn
                    || !entry
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::ToolCall { .. } | Part::ToolResult { .. }))
                {
                    continue;
                }
                let returned = entry
                    .parts
                    .iter()
                    .any(|part| matches!(part, Part::ToolResult { .. }));
                let wire = serde_json::to_vec(&entry).unwrap();
                assert!(wire.len() < 8 * 1024, "bounded entry: {} bytes", wire.len());
                assert!(!String::from_utf8(wire).unwrap().contains(&contents));
                streamed.push(entry);
                if returned {
                    return streamed;
                }
            }
        })
        .await
        .expect("bounded tool entry deadline");
        let page = finished(&client, session.id, turn).await;
        let Output::TurnHistory(history) = client
            .execute(client.prepare(Command::ReadTurn {
                session: session.id,
                turn,
                expected_revision: page.revision,
                before: None,
                limit: 100,
            }))
            .await
            .unwrap()
        else {
            panic!("turn history expected")
        };
        assert!(streamed.iter().any(|entry| {
            entry
                .parts
                .iter()
                .any(|part| matches!(part, Part::ToolCall { .. }))
        }));
        for entry in streamed {
            assert_eq!(
                history
                    .entries
                    .iter()
                    .find(|read| read.sequence == entry.sequence),
                Some(&entry)
            );
        }
        drop(updates);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
