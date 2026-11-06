use super::*;
use std::path::Path;

fn init(path: &Path) -> git2::Repository {
    let repository = git2::Repository::init(path).unwrap();
    std::fs::write(path.join("marker.txt"), "original checkout").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(Path::new("marker.txt")).unwrap();
    index.write().unwrap();
    let tree = repository.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = git2::Signature::now("Fixture", "fixture@example.test").unwrap();
    repository
        .commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
        .unwrap();
    drop(tree);
    repository
}

fn checkout(repository: &git2::Repository, path: &Path, name: &str) {
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    let branch = repository.branch(name, &commit, false).unwrap();
    let reference = branch.into_reference();
    let mut options = git2::WorktreeAddOptions::new();
    options.reference(Some(&reference));
    repository.worktree(name, path, Some(&options)).unwrap();
}

async fn create_session(client: &Client, project: ProjectId, worktree: WorktreeId) -> Session {
    let Output::Session(session) = client
        .execute(client.prepare(Command::CreateSession {
            project: Some(project),
            worktree: Some(worktree),
            config: Some(SessionConfig {
                features: Default::default(),
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::High,
                mode: WorkMode::Code,
                options: Default::default(),
                permission: Permission::Ask,
                credential: None,
            }),
        }))
        .await
        .unwrap()
    else {
        panic!("session expected")
    };
    session
}

fn update(project: &Project, path: &Path) -> Command {
    Command::UpdateProject {
        expected: project.clone(),
        name: "Relinked".into(),
        path: path.to_str().unwrap().into(),
        appearance: project.appearance.clone(),
    }
}

async fn read(client: &Client, tree: WorktreeId) -> String {
    let Output::FileContent(file) = client
        .execute(client.prepare(Command::ReadFile {
            worktree: tree,
            path: "marker.txt".into(),
        }))
        .await
        .unwrap()
    else {
        panic!("file expected")
    };
    file.text
}

#[tokio::test]
async fn retains_branch_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let node = Node::start_empty(root.join("node")).await.unwrap();
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
        let original_path = root.join(format!("original-{index}"));
        let repository = init(&original_path);
        let Output::Project(project) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Original".into(),
                path: original_path.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        let main = snapshot(&client)
            .await
            .worktrees
            .into_iter()
            .find(|tree| tree.project == Some(project.id) && tree.main)
            .unwrap();
        let branch_path = root.join(format!("branch-{index}"));
        checkout(&repository, &branch_path, "retained-branch");
        let Output::Worktree(branch) = client
            .execute(client.prepare(Command::RegisterWorktree {
                project: project.id,
                path: branch_path.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("worktree expected")
        };
        let main_session = create_session(&client, project.id, main.id).await;
        let branch_session = create_session(&client, project.id, branch.id).await;
        let unrelated = root.join(format!("unrelated-{index}"));
        init(&unrelated);
        assert_eq!(
            client
                .execute(client.prepare(update(&project, &unrelated)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        let unchanged = snapshot(&client).await;
        assert!(unchanged.projects.contains(&project));
        assert!(unchanged.worktrees.contains(&main));
        assert!(unchanged.worktrees.contains(&branch));

        let next_path = root.join(format!("next-{index}"));
        checkout(&repository, &next_path, "next-main");
        std::fs::write(next_path.join("marker.txt"), "new main checkout").unwrap();
        let request = client.prepare(update(&project, &next_path.join(".")));
        let result = client.execute(request.clone()).await.unwrap();
        assert_eq!(client.execute(request).await.unwrap(), result);
        let Output::Project(updated) = result else {
            panic!("project expected")
        };
        assert_eq!(updated.id, project.id);
        assert_eq!(updated.path, next_path.to_str().unwrap());
        let state = snapshot(&client).await;
        assert!(state.worktrees.contains(&branch));
        assert!(state.sessions.contains(&main_session));
        assert!(state.sessions.contains(&branch_session));
        let current_main = state
            .worktrees
            .iter()
            .find(|tree| tree.id == main.id)
            .unwrap();
        assert_eq!(current_main.project, Some(project.id));
        assert!(current_main.main);
        assert_eq!(current_main.path, updated.path);
        assert_eq!(
            read(&client, main_session.worktree).await,
            "new main checkout"
        );
        assert_eq!(
            read(&client, branch_session.worktree).await,
            "original checkout"
        );
        assert_eq!(
            std::fs::read_to_string(original_path.join("marker.txt")).unwrap(),
            "original checkout"
        );
        assert_eq!(
            client
                .execute(client.prepare(update(&project, &original_path)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );

        let Output::Project(other) = client
            .execute(client.prepare(Command::RegisterProject {
                name: "Other".into(),
                path: unrelated.to_str().unwrap().into(),
            }))
            .await
            .unwrap()
        else {
            panic!("project expected")
        };
        for (path, code) in [
            (
                root.join(format!("missing-{index}")),
                ErrorCode::InvalidRequest,
            ),
            (next_path.join("marker.txt"), ErrorCode::InvalidRequest),
            (
                std::path::PathBuf::from("relative"),
                ErrorCode::InvalidRequest,
            ),
            (std::path::PathBuf::from(&other.path), ErrorCode::Conflict),
        ] {
            assert_eq!(
                client
                    .execute(client.prepare(update(&updated, &path)))
                    .await
                    .unwrap_err()
                    .code,
                code
            );
            let unchanged = snapshot(&client).await;
            assert!(unchanged.projects.contains(&updated));
            assert!(unchanged.worktrees.contains(current_main));
            assert!(unchanged.worktrees.contains(&branch));
        }
    }
    controller.close().await.unwrap();
    node.shutdown().await.unwrap();
}
