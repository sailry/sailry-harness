use sailry_client::Client;
use sailry_link::{Link, NetworkScope};
use sailry_node_runtime::Node;
use sailry_protocol::{projects::*, *};

#[tokio::test]
async fn creation_and_clone() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source = root.join("source");
    let repository = git2::Repository::init(&source).unwrap();
    std::fs::write(source.join("README.md"), "fixture").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(std::path::Path::new("README.md")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
    let commit = repository
        .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
        .unwrap();
    repository
        .branch("fixture", &repository.find_commit(commit).unwrap(), false)
        .unwrap();
    let profile = root.join("node");
    let node = Node::start(&profile).await.unwrap();
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
        let folder = root.join(format!("folder-{index}"));
        std::fs::create_dir(&folder).unwrap();
        for (clone, path) in [(false, folder), (true, root.join(format!("clone-{index}")))] {
            let draft = Draft {
                name: format!("Project {index} {clone}"),
                path: path.to_str().unwrap().into(),
                appearance: Appearance {
                    icon: "code".into(),
                    color: "violet".into(),
                },
                source: if clone {
                    Source::Clone {
                        url: source.to_str().unwrap().into(),
                        branch: Some("fixture".into()),
                    }
                } else {
                    Source::Local
                },
            };
            let request = client.prepare(Command::CreateProject(draft.clone()));
            let result = client.execute(request.clone()).await.unwrap();
            assert_eq!(client.execute(request).await.unwrap(), result);
            let Output::Project(project) = result else {
                panic!("project expected")
            };
            assert_eq!(project.appearance, draft.appearance);
            assert_eq!(project.path, draft.path);
            if clone {
                assert_eq!(
                    std::fs::read_to_string(path.join("README.md")).unwrap(),
                    "fixture"
                );
                assert_eq!(
                    git2::Repository::open(path)
                        .unwrap()
                        .head()
                        .unwrap()
                        .shorthand()
                        .unwrap(),
                    "fixture"
                );
            }
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert!(snapshot.projects.contains(&project));
            assert!(
                snapshot
                    .worktrees
                    .iter()
                    .any(|tree| tree.project == Some(project.id) && tree.main)
            );
            let error = client
                .execute(client.prepare(Command::CreateProject(draft)))
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Conflict);
        }
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
    let node = Node::start(&profile).await.unwrap();
    let client = Client::new(node.local());
    let Output::Snapshot(snapshot) = client
        .execute(client.prepare(Command::Snapshot))
        .await
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    assert_eq!(snapshot.projects.len(), 4);
    assert!(
        snapshot
            .projects
            .iter()
            .all(|project| project.appearance.color == "violet")
    );
    node.shutdown().await.unwrap();
}
