use super::*;

#[tokio::test]
async fn pages_with_ignore_errors_and_exclusions() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(project.join(".gitignore")).unwrap();
    std::fs::create_dir_all(project.join("node_modules")).unwrap();
    for name in ["a.rs", "b.rs", "node_modules/hidden.rs"] {
        std::fs::write(project.join(name), "needle\n").unwrap();
    }
    let node = Node::start_empty(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invite = node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&local, &remote] {
        let globs = vec!["*.rs".into(), "!node_modules".into()];
        let mut found = std::collections::BTreeSet::new();
        for offset in 0..2 {
            let Output::FileNames(result) = client
                .execute(client.prepare(Command::FindFiles {
                    worktree: id,
                    globs: globs.clone(),
                    path: String::new(),
                    limit: 1,
                    offset,
                }))
                .await
                .unwrap()
            else {
                panic!("file names expected")
            };
            assert!(!result.partial);
            assert!(!result.diagnostics.is_empty());
            assert_eq!(result.next_offset, (offset == 0).then_some(1));
            found.extend(result.paths);
            let Output::SearchResults(result) = client
                .execute(client.prepare(Command::SearchFiles {
                    worktree: id,
                    options: FileSearch {
                        query: "needle".into(),
                        globs: globs.clone(),
                        limit: 1,
                        offset,
                        context: 0,
                        ..Default::default()
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("search results expected")
            };
            assert!(!result.partial);
            assert!(!result.diagnostics.is_empty());
            assert_eq!(result.next_offset, (offset == 0).then_some(1));
            assert_eq!(result.matches.len(), 1);
            assert_eq!(result.matches[0].path, ["a.rs", "b.rs"][offset]);
        }
        assert_eq!(
            found,
            std::collections::BTreeSet::from(["a.rs".into(), "b.rs".into()])
        );
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}

#[tokio::test]
async fn preserves_context_and_pages_large_requests() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let text = (1..=401)
        .map(|line| {
            if line == 201 {
                "hit\n".into()
            } else {
                format!("line {line}\n")
            }
        })
        .collect::<String>();
    std::fs::write(project.join("context.txt"), text).unwrap();
    let lines = "line\n".repeat(MAX_FILE_LINES + 2);
    std::fs::write(project.join("lines.txt"), &lines).unwrap();
    for index in 0..=MAX_FIND_MATCHES {
        std::fs::write(project.join(format!("{index:04}.rs")), "").unwrap();
    }
    let node = Node::start_empty(temp.path().join("node")).await.unwrap();
    let local = Client::new(node.local());
    let id = worktree(&local, &project).await;
    let controller = Link::controller(temp.path().join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let invite = node.link().invite().unwrap();
    let address = controller.handle().pair(invite.ticket()).await.unwrap();
    let remote = Client::new(controller.handle().remote(address));
    for client in [&local, &remote] {
        for context in [6, 8] {
            let Output::SearchResults(result) = client
                .execute(client.prepare(Command::SearchFiles {
                    worktree: id,
                    options: FileSearch {
                        query: "hit".into(),
                        path: "context.txt".into(),
                        context,
                        limit: 1_000_000,
                        ..Default::default()
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("search results expected")
            };
            assert_eq!(result.matches.len(), 2 * context + 1);
            assert_eq!(result.matches.first().unwrap().line_number, 201 - context);
            assert_eq!(result.matches.last().unwrap().line_number, 201 + context);
            assert_eq!(result.matches.iter().filter(|row| row.matched).count(), 1);
            assert_eq!(result.next_offset, None);
        }
        let mut offset = 0;
        let mut numbers = Vec::new();
        loop {
            let Output::SearchResults(result) = client
                .execute(client.prepare(Command::SearchFiles {
                    worktree: id,
                    options: FileSearch {
                        query: "hit".into(),
                        path: "context.txt".into(),
                        context: 1000,
                        limit: 1_000_000,
                        offset,
                        ..Default::default()
                    },
                }))
                .await
                .unwrap()
            else {
                panic!("search results expected")
            };
            assert!(!result.partial);
            assert!(result.matches.len() <= MAX_SEARCH_MATCHES);
            numbers.extend(result.matches.iter().map(|row| row.line_number));
            let Some(next) = result.next_offset else {
                break;
            };
            assert_eq!(next, offset + result.matches.len());
            offset = next;
        }
        assert_eq!(numbers, (1..=401).collect::<Vec<_>>());
        let mut offset = 0;
        let mut read = String::new();
        loop {
            let Output::FileLines(result) = client
                .execute(client.prepare(Command::ReadFileLines {
                    worktree: id,
                    path: "lines.txt".into(),
                    offset,
                    limit: 1_000_000,
                    expected_revision: None,
                }))
                .await
                .unwrap()
            else {
                panic!("file lines expected")
            };
            assert!(result.lines.len() <= MAX_FILE_LINES);
            read.extend(result.lines.iter().map(|line| line.text.as_str()));
            let Some(next) = result.next_offset else {
                break;
            };
            assert_eq!(next, offset + result.lines.len());
            offset = next;
        }
        assert_eq!(read, lines);
        let mut offset = 0;
        let mut paths = std::collections::BTreeSet::new();
        loop {
            let Output::FileNames(result) = client
                .execute(client.prepare(Command::FindFiles {
                    worktree: id,
                    globs: vec!["*.rs".into()],
                    path: String::new(),
                    limit: 1_000_000,
                    offset,
                }))
                .await
                .unwrap()
            else {
                panic!("file names expected")
            };
            assert!(result.paths.len() <= MAX_FIND_MATCHES);
            for path in &result.paths {
                assert!(paths.insert(path.clone()));
            }
            let Some(next) = result.next_offset else {
                break;
            };
            assert_eq!(next, offset + result.paths.len());
            offset = next;
        }
        assert_eq!(paths.len(), MAX_FIND_MATCHES + 1);
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
