use super::*;
#[path = "../../support/skills.rs"]
mod github;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn executes_retained_import() {
    for remote in [false, true] {
        let github = github::Github::start().await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        fs::create_dir_all(&root).unwrap();
        let node = Node::start_with_skill_source(directory.path().join("node"), &github.endpoint)
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
        let Output::SkillDiscovery(discovery) = execute(
            &client,
            Command::DiscoverSkills {
                source: github::source(),
            },
        )
        .await
        else {
            panic!("skill discovery expected")
        };
        let selected = discovery
            .skills
            .iter()
            .find(|candidate| candidate.skill.name == "analysis")
            .unwrap();
        let Output::Plugin(installed) =
            execute(&client, github::install(&discovery, selected, 0)).await
        else {
            panic!("plugin expected")
        };
        let skill = format!("{}:analysis", installed.summary.name);
        let script = node
            .profile()
            .join("plugins/packages")
            .join(&installed.summary.digest)
            .join("skills/analysis/scripts/check.sh");
        let command = format!("sh '{}'", script.to_str().unwrap().replace('\'', "'\\''"));
        let calls = vec![
            ("load_skill".into(), json!({"skill": skill})),
            (
                "read_skill_resource".into(),
                json!({"skill": skill, "path": "references/guide.md"}),
            ),
            (
                plugin_tool("commands", "run_command"),
                json!({"command": command}),
            ),
        ];
        let server = Server::tools(calls.clone()).await;
        let session = approvals::prepare(&client, &server, &root).await;
        enabled_only(&client, &["commands", &installed.summary.name]).await;
        let Output::QueuedTurn(turn) = execute(
            &client,
            Command::QueueTurn {
                session: session.id,
                expected_revision: session.revision,
                message: "Read the skill and run its script".into(),
            },
        )
        .await
        else {
            panic!("queued turn expected")
        };
        let Output::Plugin(commands) = execute(
            &client,
            Command::ReadPlugin {
                name: "commands".into(),
            },
        )
        .await
        else {
            panic!("commands package expected")
        };
        assert_eq!(
            turn.plugins,
            [commands.summary.reference(), installed.summary.reference()]
        );
        github.updated.store(true, Ordering::SeqCst);
        let Output::SkillDiscovery(updated) = execute(
            &client,
            Command::DiscoverSkills {
                source: github::source(),
            },
        )
        .await
        else {
            panic!("skill discovery expected")
        };
        assert_eq!(updated.source.commit, github::SECOND);
        let next = updated
            .skills
            .iter()
            .find(|candidate| candidate.name == selected.name)
            .unwrap();
        let Output::Plugin(updated) = execute(&client, github::install(&updated, next, 1)).await
        else {
            panic!("plugin expected")
        };
        assert_ne!(updated.summary.digest, installed.summary.digest);
        execute(
            &client,
            Command::RemovePlugin {
                name: selected.name.clone(),
                expected_revision: 2,
            },
        )
        .await;
        assert_eq!(github.requests.lock().unwrap().len(), 6);
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        let Output::Plugin(retained) = execute(
            &client,
            Command::ReadPluginVersion {
                package: installed.summary.reference(),
            },
        )
        .await
        else {
            panic!("plugin version expected")
        };
        assert_eq!(retained.skill, installed.skill);
        execute(&client, Command::StartQueuedTurn { turn: turn.id }).await;
        let (_, approval) = approvals::pending(&client, session.id).await;
        assert!(!root.join("script-count.txt").exists());
        execute(
            &client,
            Command::ResolveApproval {
                session: session.id,
                approval: approval.id,
                decision: Decision::Approve,
            },
        )
        .await;
        let page = finished(&client, session.id, turn.id).await;
        assert_eq!(page.runs[0].status, Status::Completed, "{:?}", page.runs);
        super::super::evaluation::trajectory(&page, &calls);
        let output = results(&page);
        assert_eq!(output[0]["content"], github::body("first"));
        assert_eq!(output[1]["content"], "Guide first 中文 🙂");
        assert_eq!(
            fs::read_to_string(root.join("script-count.txt")).unwrap(),
            "x"
        );
        assert!(
            process::results(&page)[0]
                .stdout
                .text
                .contains("script first")
        );
        let requests = server.requests.lock().unwrap().clone();
        assert!(requests[0]["messages"].to_string().contains(&skill));
        assert!(
            !requests[0]["messages"]
                .to_string()
                .contains("Version first")
        );
        assert_eq!(requests.len(), 4);
        drop(client);
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
