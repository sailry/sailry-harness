use super::super::{Target, locate};
use core::prelude::v1::test;
use sailry_client::Client;
use sailry_link::CancellationToken;
use sailry_protocol::{Command, ErrorCode, Output, plugin};

#[test]
fn reads_the_execution_nodes_registered_files() {
    let fixture = crate::activity::fixture::Fixture::new();
    for index in 0..2 {
        let transport = if index == 0 {
            fixture.nodes[0].local()
        } else {
            fixture.nodes[0]
                .link()
                .remote(fixture.nodes[1].link().address())
        };
        let client = Client::new(transport);
        fixture.runtime.block_on(async {
            let Output::Session(session) = client
                .execute(client.prepare(Command::CreateSession {
                    project: None,
                    worktree: None,
                    config: Some(fixture.sessions[index].config.clone()),
                }))
                .await
                .unwrap()
            else {
                panic!("session expected")
            };
            assert!(session.project.is_none());
            assert!(!session.archived);
            let path = "THEME_DEVELOPMENT_SPEC.md";
            for (worktree, text) in [
                (session.worktree, "Original session document"),
                (
                    fixture.sessions[index].worktree,
                    "Registered project document",
                ),
            ] {
                client
                    .execute(client.prepare(Command::WriteFile {
                        worktree,
                        path: path.into(),
                        text: text.into(),
                        expected_revision: None,
                    }))
                    .await
                    .unwrap();
            }
            let Output::Snapshot(snapshot) = client
                .execute(client.prepare(Command::Snapshot))
                .await
                .unwrap()
            else {
                panic!("snapshot expected")
            };
            assert_eq!(snapshot.node, client.target());
            let Output::Plugin(package) = client
                .execute(client.prepare(Command::ReadPlugin {
                    name: "files".into(),
                }))
                .await
                .unwrap()
            else {
                panic!("file renderer expected")
            };
            let mut context = plugin::Context {
                invocation: None,
                turn: None,
                surface: plugin::desktop::Surface::Workspace,
                package: package.summary.reference(),
                worktree: Some(session.worktree),
                session: Some(session.id),
            };
            let Some((
                owner,
                Target::Workspace {
                    path: relative,
                    line,
                },
            )) = locate(&format!("{path}#L2"), session.worktree, &snapshot.worktrees)
            else {
                panic!("session file expected")
            };
            assert_eq!(owner, session.worktree);
            assert_eq!(line, Some(2));
            let content = client
                .read_document_scoped(&context, &relative, CancellationToken::new())
                .await
                .unwrap();
            assert_eq!(content.text, "Original session document");

            let project = snapshot
                .worktrees
                .iter()
                .find(|entry| entry.id == fixture.sessions[index].worktree)
                .unwrap();
            for link in [
                format!("{}/{path}#L2", project.path),
                format!("file://{}/{path}#L2", project.path),
            ] {
                let Some((
                    owner,
                    Target::Workspace {
                        path: relative,
                        line,
                    },
                )) = locate(&link, session.worktree, &snapshot.worktrees)
                else {
                    panic!("registered project file expected")
                };
                assert_eq!(owner, project.id);
                assert_eq!(line, Some(2));
                context.worktree = Some(owner);
                context.session = Some(session.id);
                assert_eq!(
                    client
                        .read_document_scoped(&context, &relative, CancellationToken::new())
                        .await
                        .unwrap_err()
                        .code,
                    ErrorCode::PermissionDenied
                );
                context.session = None;
                let content = client
                    .read_document_scoped(&context, &relative, CancellationToken::new())
                    .await
                    .unwrap();
                assert_eq!(content.path, path);
                assert_eq!(content.text, "Registered project document");
            }
            assert_eq!(
                client
                    .read_document_scoped(&context, "../unregistered.md", CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidRequest
            );
            let foreign = fixture.sessions[1 - index].worktree;
            assert_eq!(locate(path, foreign, &snapshot.worktrees), None);
            assert_eq!(
                client
                    .read_document(foreign, path, CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::NotFound
            );
        });
    }
    fixture.close();
}
