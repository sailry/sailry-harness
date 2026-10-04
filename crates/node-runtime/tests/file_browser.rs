use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{file_browser::*, *};

async fn manage(client: &Client, action: Action) -> Vec<Outcome> {
    let request = client.prepare(Command::ManageFiles(action));
    let output = client.execute(request.clone()).await.unwrap();
    assert_eq!(client.execute(request).await.unwrap(), output);
    let Output::FilesManaged(results) = output else {
        panic!("file outcomes expected")
    };
    results
}

#[tokio::test]
async fn local_and_remote() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let node = Node::start(root.join("node")).await.unwrap();
    let controller = Link::controller(root.join("controller"), NetworkScope::default())
        .await
        .unwrap();
    let address = controller
        .handle()
        .pair(node.link().invite().unwrap().ticket())
        .await
        .unwrap();
    for (index, transport) in [node.local(), controller.handle().remote(address)]
        .into_iter()
        .enumerate()
    {
        let client = Client::new(transport);
        let directory = root.join(format!("files-{index}"));
        std::fs::create_dir(&directory).unwrap();
        let path = |name: &str| directory.join(name).to_str().unwrap().to_owned();
        let created = manage(
            &client,
            Action::CreateDirectory {
                path: path("target"),
            },
        )
        .await;
        assert!(created[0].error.is_none());
        std::fs::write(path("资料.txt"), "fixture").unwrap();
        let Output::FileListing(listing) = client
            .execute(client.prepare(Command::BrowseFiles {
                directory: Some(path("")),
                after: None,
            }))
            .await
            .unwrap()
        else {
            panic!("listing expected")
        };
        assert_eq!(listing.directory.entries.len(), 2);
        assert_eq!(listing.directory.entries[0].kind, EntryKind::Directory);
        assert_eq!(listing.directory.path, directory.to_str().unwrap());
        assert!(!listing.locations.is_empty());
        assert_eq!(listing.parent, Some(root.to_str().unwrap().into()));
        let copied = manage(
            &client,
            Action::Transfer {
                paths: vec![path("资料.txt")],
                destination: path("target"),
                cut: false,
                conflict: Conflict::Stop,
            },
        )
        .await;
        assert!(copied[0].error.is_none());
        let conflict = manage(
            &client,
            Action::Transfer {
                paths: vec![path("资料.txt")],
                destination: path("target"),
                cut: false,
                conflict: Conflict::Stop,
            },
        )
        .await;
        assert_eq!(
            conflict[0].error.as_ref().unwrap().code,
            ErrorCode::Conflict
        );
        let retained = manage(
            &client,
            Action::Transfer {
                paths: vec![path("资料.txt")],
                destination: path("target"),
                cut: false,
                conflict: Conflict::KeepBoth,
            },
        )
        .await;
        assert_eq!(retained[0].destination, Some(path("target/资料 (2).txt")));
        let renamed = manage(
            &client,
            Action::Rename {
                from: path("资料.txt"),
                to: path("renamed.txt"),
            },
        )
        .await;
        assert!(renamed[0].error.is_none());
        let moved = manage(
            &client,
            Action::Transfer {
                paths: vec![path("renamed.txt")],
                destination: path("target"),
                cut: true,
                conflict: Conflict::Stop,
            },
        )
        .await;
        assert!(moved[0].error.is_none());
        assert!(!std::path::Path::new(&path("renamed.txt")).exists());
        std::fs::write(path("资料.txt"), "replacement").unwrap();
        let replaced = manage(
            &client,
            Action::Transfer {
                paths: vec![path("资料.txt")],
                destination: path("target"),
                cut: false,
                conflict: Conflict::Replace,
            },
        )
        .await;
        assert!(replaced[0].error.is_none());
        assert_eq!(
            std::fs::read_to_string(path("target/资料.txt")).unwrap(),
            "replacement"
        );
        let removed = manage(
            &client,
            Action::Trash {
                paths: vec![path("target/renamed.txt")],
            },
        )
        .await;
        assert!(removed[0].error.is_none());
        assert!(!std::path::Path::new(&path("target/renamed.txt")).exists());
        let protected = client
            .execute(client.prepare(Command::ManageFiles(Action::Trash {
                paths: vec![root.join("node").to_str().unwrap().into()],
            })))
            .await
            .unwrap_err();
        assert_eq!(protected.code, ErrorCode::Conflict);
        assert!(root.join("node").is_dir());
        let Output::Snapshot(snapshot) = client
            .execute(client.prepare(Command::Snapshot))
            .await
            .unwrap()
        else {
            panic!("snapshot expected")
        };
        assert!(snapshot.projects.is_empty());
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
