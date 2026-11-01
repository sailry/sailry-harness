use super::sessions::{create, execute, provider, queue, snapshot};
use super::*;

fn team(key: &str, members: Vec<RoleId>) -> team::Profile {
    team::Profile {
        id: TeamId::new(),
        revision: 0,
        key: key.into(),
        name: "Reviewers 中文 🙂".into(),
        appearance: None,
        description: "Review delegated work".into(),
        model: None,
        instructions: "Coordinate the reviewers 中文 🙂".into(),
        members,
    }
}

async fn save(
    client: &Client,
    team: team::Profile,
    expected_revision: u64,
) -> Result<team::Profile, Fault> {
    match client
        .execute(client.prepare(Command::SaveTeam {
            team,
            expected_revision,
        }))
        .await?
    {
        Output::Team(team) => Ok(team),
        _ => panic!("team expected"),
    }
}

async fn teams(client: &Client) -> Vec<team::Profile> {
    let Output::Teams(teams) = execute(client, Command::ListTeams).await else {
        panic!("teams expected")
    };
    teams
}

async fn revise(client: &Client, command: Command) -> Result<Session, Fault> {
    match client.execute(client.prepare(command)).await? {
        Output::Session(session) => Ok(session),
        _ => panic!("session expected"),
    }
}

#[tokio::test]
async fn prunes_removed_members() {
    for remote in [false, true] {
        let (directory, node, controller, client) = fixture(remote).await;
        let mut updates = client.subscribe().await.unwrap();
        let mut projection = Projection::new(node.id(), 1);
        assert_eq!(
            projection.apply(1, updates.next().await.unwrap()).unwrap(),
            Apply::Applied
        );
        let removed = put(&client, profile("review"), 0).await.unwrap();
        let kept = put(&client, profile("analysis"), 0).await.unwrap();
        assert!(teams(&client).await.is_empty());
        let request = client.prepare(Command::SaveTeam {
            team: team("reviewers", vec![removed.id, kept.id]),
            expected_revision: 0,
        });
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        drop(admitted);
        let Output::Team(saved) = client.execute(request.clone()).await.unwrap() else {
            panic!("team expected")
        };
        assert_eq!(saved.revision, 1);
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Team(saved.clone())
        );
        assert_eq!(
            save(&client, saved.clone(), 0).await.unwrap_err().code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            save(&client, team("reviewers", vec![]), 0)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            save(&client, team("unknown", vec![RoleId::new()]), 0)
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let read = client
            .dispatch(client.prepare(Command::ListTeams))
            .await
            .unwrap();
        assert!(!read.receipt.durable);
        assert_eq!(
            read.completion.await.unwrap().unwrap(),
            Output::Teams(vec![saved.clone()])
        );
        execute(
            &client,
            Command::RemoveRole {
                role: removed.id,
                expected_revision: removed.revision,
            },
        )
        .await;
        let mut pruned = saved.clone();
        pruned.members = vec![kept.id];
        pruned.revision = 2;
        assert_eq!(teams(&client).await, [pruned.clone()]);
        tokio::time::timeout(Duration::from_secs(5), async {
            while projection.snapshot().unwrap().teams.as_slice() != std::slice::from_ref(&pruned) {
                assert_eq!(
                    projection.apply(1, updates.next().await.unwrap()).unwrap(),
                    Apply::Applied
                );
            }
        })
        .await
        .unwrap();
        drop(updates);
        node.shutdown().await.unwrap();
        let node = Node::start_empty(directory.path().join("node"))
            .await
            .unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(snapshot(&client).await.teams, [pruned.clone()]);
        assert_eq!(
            client
                .execute(client.prepare(Command::RemoveTeam {
                    team: pruned.id,
                    expected_revision: 1,
                }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        assert_eq!(
            execute(
                &client,
                Command::RemoveTeam {
                    team: pruned.id,
                    expected_revision: 2,
                },
            )
            .await,
            Output::Teams(vec![])
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn selects_future_rosters() {
    for remote in [false, true] {
        let (directory, node, controller, client) = fixture(remote).await;
        let provider = provider(&client, None).await;
        let member = put(&client, profile("review"), 0).await.unwrap();
        let outsider = put(&client, profile("analysis"), 0).await.unwrap();
        let session = create(&client, directory.path(), &provider).await;
        assert_eq!(session.roles.team, None);
        assert_eq!(session.roles.profiles, list(&client).await);
        let reviewers = save(&client, team("reviewers", vec![member.id]), 0)
            .await
            .unwrap();
        let first = queue(&client, &session).await;
        assert_eq!(first.roles, session.roles);
        let stale = team::Reference {
            id: reviewers.id,
            revision: 0,
        };
        for (team, expected) in [
            (Some(stale), ErrorCode::RevisionConflict),
            (
                Some(team::Reference {
                    id: TeamId::new(),
                    revision: 1,
                }),
                ErrorCode::NotFound,
            ),
        ] {
            assert_eq!(
                revise(
                    &client,
                    Command::SetSessionTeam {
                        session: session.id,
                        expected_revision: session.revision,
                        team,
                    },
                )
                .await
                .unwrap_err()
                .code,
                expected
            );
        }
        let request = client.prepare(Command::SetSessionTeam {
            session: session.id,
            expected_revision: session.revision,
            team: Some(reviewers.reference()),
        });
        let admitted = client.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        drop(admitted);
        let Output::Session(selected) = client.execute(request).await.unwrap() else {
            panic!("session expected")
        };
        assert_eq!(selected.revision, session.revision + 1);
        assert_eq!(selected.roles.profiles, [member.clone()]);
        assert_eq!(selected.roles.team, Some(reviewers.reference()));
        assert_eq!(selected.roles.instructions, reviewers.instructions);
        assert!(selected.roles.models.is_empty());
        let chosen = role::Model {
            provider: provider.id,
            model: "fixture".into(),
            effort: Some(Effort::Low),
        };
        for (role, model, expected) in [
            (outsider.id, chosen.clone(), ErrorCode::NotFound),
            (
                member.id,
                role::Model {
                    model: "missing".into(),
                    ..chosen.clone()
                },
                ErrorCode::NotConfigured,
            ),
        ] {
            assert_eq!(
                revise(
                    &client,
                    Command::SetSessionMemberModel {
                        session: session.id,
                        expected_revision: selected.revision,
                        role,
                        model: Some(model),
                    },
                )
                .await
                .unwrap_err()
                .code,
                expected
            );
        }
        let overridden = revise(
            &client,
            Command::SetSessionMemberModel {
                session: session.id,
                expected_revision: selected.revision,
                role: member.id,
                model: Some(chosen.clone()),
            },
        )
        .await
        .unwrap();
        assert_eq!(overridden.roles.models.get(&member.id), Some(&chosen));
        assert_eq!(overridden.roles.providers.len(), 1);
        assert_eq!(overridden.roles.providers[0].id, provider.id);
        assert_eq!(overridden.roles.profiles, selected.roles.profiles);
        let second = queue(&client, &overridden).await;
        assert_eq!(second.roles, overridden.roles);
        let Output::QueuedMessage(queued) =
            execute(&client, Command::ReadQueuedTurn { turn: first.id }).await
        else {
            panic!("queued message expected")
        };
        assert_eq!(queued.turn.roles, session.roles);
        let cleared = revise(
            &client,
            Command::SetSessionMemberModel {
                session: session.id,
                expected_revision: overridden.revision,
                role: member.id,
                model: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(cleared.roles, selected.roles);
        let overridden = revise(
            &client,
            Command::SetSessionMemberModel {
                session: session.id,
                expected_revision: cleared.revision,
                role: member.id,
                model: Some(chosen),
            },
        )
        .await
        .unwrap();
        let default = revise(
            &client,
            Command::SetSessionTeam {
                session: session.id,
                expected_revision: overridden.revision,
                team: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(default.roles.team, None);
        assert!(default.roles.instructions.is_empty());
        assert!(default.roles.models.is_empty());
        assert_eq!(default.roles.profiles, list(&client).await);
        let Output::QueuedTurn(started) = execute(
            &client,
            Command::StartSession(conversation::Start {
                project: session.project,
                worktree: Some(session.worktree),
                config: None,
                title: "Team start".into(),
                message: "Start with a team".into(),
                team: Some(reviewers.reference()),
                models: Default::default(),
            }),
        )
        .await
        else {
            panic!("turn expected")
        };
        assert_eq!(started.roles.profiles, [member]);
        assert_eq!(started.roles.team, Some(reviewers.reference()));
        assert_eq!(started.roles.instructions, reviewers.instructions);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
