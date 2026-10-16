use super::*;
use std::collections::{BTreeMap, BTreeSet};

async fn roles(fixture: &mut Fixture, servers: &[(&str, &Server)]) -> Vec<role::Profile> {
    let Output::Providers(providers) = execute(&fixture.client, Command::ListProviders).await
    else {
        panic!("providers expected")
    };
    let original = providers
        .into_iter()
        .find(|provider| provider.id == fixture.session.config.provider)
        .unwrap();
    let mut roles = Vec::new();
    for (key, server) in servers {
        let mut provider = original.clone();
        provider.id = ProviderId::new();
        provider.revision = 0;
        provider.endpoint = server.endpoint.clone();
        let Output::Provider(provider) = execute(
            &fixture.client,
            Command::PutProvider {
                provider,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("provider expected")
        };
        let role = role::Profile {
            id: RoleId::new(),
            revision: 0,
            key: (*key).into(),
            name: (*key).into(),
            appearance: None,
            description: "Review the assigned complete group".into(),
            read_only: false,
            model: Some(role::Model {
                provider: provider.id,
                model: "fixture-a".into(),
                effort: None,
            }),
            max_turns: Some(5),
            skills: vec![],
            instructions: "Report the requested review evidence".into(),
        };
        let Output::Role(role) = execute(
            &fixture.client,
            Command::PutRole {
                role,
                expected_revision: 0,
            },
        )
        .await
        else {
            panic!("role expected")
        };
        roles.push(role);
    }
    let Output::Session(session) = execute(
        &fixture.client,
        Command::SetSessionRoles {
            session: fixture.session.id,
            expected_revision: fixture.session.revision,
            roles: roles.iter().map(role::Profile::reference).collect(),
        },
    )
    .await
    else {
        panic!("session expected")
    };
    fixture.session = session;
    roles
}

fn group_receipt(scope: &Value, path: &str) -> Value {
    let group = scope["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| {
            group["files"]
                .as_array()
                .unwrap()
                .iter()
                .any(|file| file == path)
        })
        .unwrap();
    json!({"review_id":scope["review_id"],"reviewed_files":group["files"].as_array().unwrap().iter()
        .map(|path| { let path = path.as_str().unwrap(); json!({"path":path,"revision":file(scope,path)["revision"]}) })
        .collect::<Vec<_>>(),"reviewed_groups":[group["id"]],"finalize":false})
}

fn finalize(scope: &Value) -> Value {
    json!({"review_id":scope["review_id"],"reviewed_files":[],"reviewed_groups":[],"finalize":true})
}

async fn stage(fixture: &Fixture, child: SessionId, tool: &str) -> Approval {
    println!("Awaiting {tool} approval in child {child}");
    let approval = crate::approvals::rules::pending(&fixture.client, child).await;
    let page = history(&fixture.client, child).await;
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == approval.entry)
        .unwrap();
    assert!(
        matches!(entry.parts.get(approval.index), Some(Part::ToolCall { name, .. })
        if name == &plugin_tool(PACKAGE, tool)),
        "Unexpected {tool} approval: {approval:?}, {entry:?}"
    );
    approval
}

fn won(result: &Value) -> bool {
    if result["isError"] == true {
        failure(result, "revision_conflict");
        false
    } else {
        success(result);
        true
    }
}

fn group_id(scope: &Value, path: &str) -> String {
    group_receipt(scope, path)["reviewed_groups"][0]
        .as_str()
        .unwrap()
        .into()
}

async fn retained(
    fixture: &Fixture,
    package: &plugin::Info,
    scope: &Value,
    findings: &BTreeMap<String, Value>,
    groups: &BTreeMap<String, SessionId>,
) {
    let id = scope["review_id"].as_str().unwrap();
    let head = value(fixture, package, &format!("review/{id}/head"))
        .await
        .value;
    assert_eq!(head["status"], "open");
    let actual: BTreeSet<_> = head["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual.len(), head["findings"].as_array().unwrap().len());
    assert_eq!(actual, findings.keys().cloned().collect());
    for (finding_id, expected) in findings {
        let record = value(
            fixture,
            package,
            &format!("review/{id}/finding/{finding_id}"),
        )
        .await
        .value;
        assert_eq!(record["kind"], "finding");
        assert_eq!(&record["finding"], expected);
    }
    let covered: BTreeSet<_> = head["reviewed_groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        covered.len(),
        head["reviewed_groups"].as_array().unwrap().len()
    );
    assert_eq!(covered, groups.keys().cloned().collect());
    let receipts = head["receipts"].as_array().unwrap();
    assert_eq!(receipts.len(), groups.len());
    let mut reporters = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for receipt in receipts {
        let receipt = receipt.as_str().unwrap();
        assert!(ids.insert(receipt));
        let record = value(
            fixture,
            package,
            &format!("review/{id}/receipt/{receipt}/0"),
        )
        .await
        .value;
        assert_eq!(record["kind"], "receipt");
        let rows = record["rows"].as_array().unwrap();
        let group_rows: Vec<_> = rows.iter().filter(|row| row["kind"] == "group").collect();
        let reporter_rows: Vec<_> = rows
            .iter()
            .filter(|row| row["kind"] == "reporter")
            .collect();
        assert_eq!(group_rows.len(), 1);
        assert_eq!(reporter_rows.len(), 1);
        let group = group_rows[0]["value"].as_str().unwrap();
        let reporter: SessionId = reporter_rows[0]["value"].as_str().unwrap().parse().unwrap();
        assert!(reporters.insert(group.to_owned(), reporter).is_none());
        let expected = scope["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["id"] == group)
            .unwrap();
        let files: Vec<_> = rows
            .iter()
            .filter(|row| row["kind"] == "file")
            .map(|row| row["value"].clone())
            .collect();
        let expected: Vec<_> = expected["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|path| {
                let path = path.as_str().unwrap();
                json!({"path":path,"revision":file(scope,path)["revision"]})
            })
            .collect();
        assert_eq!(files, expected);
        assert_eq!(rows.len(), files.len() + 2);
    }
    assert_eq!(&reporters, groups);
}

async fn retry(
    fixture: &Fixture,
    child: SessionId,
    previous: TurnId,
    server: &Server,
    actions: Vec<(String, Value)>,
) -> Vec<Value> {
    assert!(!actions.is_empty());
    // A reused child retains its provider snapshot, including the original endpoint.
    let before = server.requests.lock().unwrap().len();
    server.replace_tools(actions.clone());
    let Output::Session(session) =
        execute(&fixture.client, Command::ReadSession { session: child }).await
    else {
        panic!("child session expected")
    };
    assert_eq!(
        session.profile.as_ref().unwrap().provider.endpoint,
        server.endpoint
    );
    let parent = Server::turn_tools(vec![(plugin_tool("delegation", "spawn_agent"),
        json!({"session":child,"task":"Retry only the conflicted review actions from the current head"}))]).await;
    route(&fixture.client, &fixture.session, &parent.endpoint).await;
    let turn = submit(&fixture.client, &fixture.session).await;
    let mut child_turn = None;
    for (name, _) in &actions {
        let tool = if *name == plugin_tool(PACKAGE, "report_finding") {
            "report_finding"
        } else {
            "review_complete"
        };
        let approval = stage(fixture, child, tool).await;
        assert_ne!(approval.turn, previous);
        if let Some(child_turn) = child_turn {
            assert_eq!(approval.turn, child_turn);
        } else {
            child_turn = Some(approval.turn);
        }
        process::decide(&fixture.client, &approval, Decision::Approve).await;
    }
    let child_turn = child_turn.unwrap();
    let parent_page = completed(&fixture.client, fixture.session.id, turn.id).await;
    let reused: Vec<_> = parent_page
        .children
        .iter()
        .filter(|child| child.origin.turn == turn.id)
        .collect();
    assert_eq!(reused.len(), 1);
    assert_eq!(reused[0].run.session, child);
    assert_eq!(reused[0].run.turn, child_turn);
    let page = completed(&fixture.client, child, child_turn).await;
    let results = crate::approvals::rules::results(&page, child_turn);
    assert_eq!(results.len(), actions.len(), "{page:?}");
    for ((_, name, result), (expected, _)) in results.iter().zip(&actions) {
        assert_eq!(name, expected);
        success(result);
    }
    assert_eq!(
        server.requests.lock().unwrap().len() - before,
        actions.len() + 1
    );
    results.into_iter().map(|(_, _, result)| result).collect()
}

#[tokio::test]
async fn retries_parallel_reports_without_lost_receipts() {
    for remote in [false, true] {
        let (mut fixture, package) = start(remote).await;
        crate::approvals::install(&fixture.client, &["delegation"]).await;
        let repository = Repository::init(&fixture.root).unwrap();
        write(&fixture.root, "alpha/value.rs", "old_alpha();\n");
        write(&fixture.root, "beta/value.py", "old_beta()\n");
        commit(&repository, "Initial groups");
        write(&fixture.root, "alpha/value.rs", "new_alpha();\n");
        write(&fixture.root, "beta/value.py", "new_beta()\n");
        let scope = scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
        assert_eq!(scope["groups"].as_array().unwrap().len(), 2);
        let head_key = format!("review/{}/head", scope["review_id"].as_str().unwrap());
        let original = value(&fixture, &package, &head_key).await;
        let first = Server::scripted_tools(vec![
            (
                plugin_tool(PACKAGE, "report_finding"),
                finding(&scope, "alpha/value.rs", "new", 1, "new_alpha();"),
            ),
            (
                plugin_tool(PACKAGE, "review_complete"),
                group_receipt(&scope, "alpha/value.rs"),
            ),
            (plugin_tool(PACKAGE, "review_complete"), finalize(&scope)),
        ])
        .await;
        let second = Server::scripted_tools(vec![
            (
                plugin_tool(PACKAGE, "report_finding"),
                finding(&scope, "beta/value.py", "new", 1, "new_beta()"),
            ),
            (
                plugin_tool(PACKAGE, "review_complete"),
                group_receipt(&scope, "beta/value.py"),
            ),
            (plugin_tool(PACKAGE, "review_complete"), finalize(&scope)),
        ])
        .await;
        let profiles = roles(&mut fixture, &[("alpha", &first), ("beta", &second)]).await;
        permission(&mut fixture, Permission::Ask).await;
        let parent = Server::turn_parallel(vec![
            (plugin_tool("delegation","spawn_agent"),json!({"role":"alpha","task":"Review the alpha group and submit its complete receipt"})),
            (plugin_tool("delegation","spawn_agent"),json!({"role":"beta","task":"Review the beta group and submit its complete receipt"})),
        ]).await;
        route(&fixture.client, &fixture.session, &parent.endpoint).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let page = observe(&fixture.client, fixture.session.id, |page| {
            page.children
                .iter()
                .filter(|child| child.origin.turn == turn.id)
                .count()
                == 2
        })
        .await;
        let child = |role: RoleId| {
            page.children
                .iter()
                .find(|child| child.origin.turn == turn.id && child.origin.role == Some(role))
                .unwrap()
                .run
                .session
        };
        let alpha = child(profiles[0].id);
        let beta = child(profiles[1].id);
        let alpha_report = stage(&fixture, alpha, "report_finding").await;
        let beta_report = stage(&fixture, beta, "report_finding").await;
        assert_ne!(alpha_report.id, beta_report.id);
        // Original confirmation precedes JS preparation; approval does not freeze the head.
        assert_eq!(value(&fixture, &package, &head_key).await, original);
        tokio::join!(
            process::decide(&fixture.client, &alpha_report, Decision::Approve),
            process::decide(&fixture.client, &beta_report, Decision::Approve),
        );
        let alpha_receipt = stage(&fixture, alpha, "review_complete").await;
        let beta_receipt = stage(&fixture, beta, "review_complete").await;
        assert_ne!(alpha_receipt.id, alpha_report.id);
        assert_ne!(beta_receipt.id, beta_report.id);
        assert_eq!(alpha_receipt.turn, alpha_report.turn);
        assert_eq!(beta_receipt.turn, beta_report.turn);
        tokio::join!(
            process::decide(&fixture.client, &alpha_receipt, Decision::Approve),
            process::decide(&fixture.client, &beta_receipt, Decision::Approve),
        );
        let alpha_finalize = stage(&fixture, alpha, "review_complete").await;
        let beta_finalize = stage(&fixture, beta, "review_complete").await;
        assert_ne!(alpha_finalize.id, alpha_receipt.id);
        assert_ne!(beta_finalize.id, beta_receipt.id);
        assert_eq!(alpha_finalize.turn, alpha_report.turn);
        assert_eq!(beta_finalize.turn, beta_report.turn);
        tokio::join!(
            process::decide(&fixture.client, &alpha_finalize, Decision::Approve),
            process::decide(&fixture.client, &beta_finalize, Decision::Approve),
        );
        completed(&fixture.client, fixture.session.id, turn.id).await;
        let alpha_page = completed(&fixture.client, alpha, alpha_report.turn).await;
        let beta_page = completed(&fixture.client, beta, beta_report.turn).await;
        let alpha_results = crate::approvals::rules::results(&alpha_page, alpha_report.turn);
        let beta_results = crate::approvals::rules::results(&beta_page, beta_report.turn);
        assert_eq!(alpha_results.len(), 3);
        assert_eq!(beta_results.len(), 3);
        failure(&alpha_results[2].2, "permission_denied");
        failure(&beta_results[2].2, "permission_denied");
        let reports = [won(&alpha_results[0].2), won(&beta_results[0].2)];
        let receipts = [won(&alpha_results[1].2), won(&beta_results[1].2)];
        assert!(reports.into_iter().any(|won| won));
        assert!(receipts.into_iter().any(|won| won));
        let children = [
            (alpha, "alpha/value.rs", alpha_report.turn, &alpha_results),
            (beta, "beta/value.py", beta_report.turn, &beta_results),
        ];
        let mut findings = BTreeMap::new();
        let mut groups = BTreeMap::new();
        for (index, (child, path, _, results)) in children.iter().enumerate() {
            if reports[index] {
                let finding = &results[0].2["finding"];
                assert_eq!(finding["path"], *path);
                assert_eq!(finding["reporter"], json!(child));
                assert!(
                    findings
                        .insert(finding["id"].as_str().unwrap().to_owned(), finding.clone())
                        .is_none()
                );
            }
            if receipts[index] {
                assert!(groups.insert(group_id(&scope, path), *child).is_none());
            }
        }
        retained(&fixture, &package, &scope, &findings, &groups).await;
        for (index, (child, path, previous, _)) in children.iter().enumerate() {
            let mut actions = Vec::new();
            if !reports[index] {
                let snippet = if index == 0 {
                    "new_alpha();"
                } else {
                    "new_beta()"
                };
                actions.push((
                    plugin_tool(PACKAGE, "report_finding"),
                    finding(&scope, path, "new", 1, snippet),
                ));
            }
            if !receipts[index] {
                actions.push((
                    plugin_tool(PACKAGE, "review_complete"),
                    group_receipt(&scope, path),
                ));
            }
            if actions.is_empty() {
                continue;
            }
            let server = if index == 0 { &first } else { &second };
            let results = retry(&fixture, *child, *previous, server, actions).await;
            let mut offset = 0;
            if !reports[index] {
                let finding = &results[offset]["finding"];
                assert_eq!(finding["path"], *path);
                assert_eq!(finding["reporter"], json!(child));
                assert!(
                    findings
                        .insert(finding["id"].as_str().unwrap().to_owned(), finding.clone())
                        .is_none()
                );
                offset += 1;
            }
            if !receipts[index] {
                assert_eq!(results[offset]["status"], "open");
                assert!(groups.insert(group_id(&scope, path), *child).is_none());
            }
        }
        assert_eq!(findings.len(), 2);
        assert_eq!(groups.len(), 2);
        retained(&fixture, &package, &scope, &findings, &groups).await;
        fixture = restart(fixture, remote).await;
        retained(&fixture, &package, &scope, &findings, &groups).await;
        permission(&mut fixture, Permission::Full).await;
        let resumed = call(
            &fixture,
            "review_scope",
            json!({"review_id":scope["review_id"]}),
        )
        .await;
        success(&resumed);
        assert_eq!(resumed["counts"]["findings"], 2);
        assert_eq!(resumed["reviewed_groups"].as_array().unwrap().len(), 2);
        let findings = resumed["findings"].as_array().unwrap();
        assert_eq!(findings.len(), 2);
        assert_ne!(findings[0]["id"], findings[1]["id"]);
        let reporters: std::collections::BTreeSet<_> = findings
            .iter()
            .map(|finding| finding["reporter"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            reporters,
            std::collections::BTreeSet::from([alpha.to_string(), beta.to_string()])
        );
        let result = call(&fixture, "review_complete", finalize(&scope)).await;
        success(&result);
        assert_eq!(result["status"], "complete");
        assert_eq!(result["findings"], 2);
        stop(fixture).await;
    }
}

#[tokio::test]
async fn retries_parallel_group_receipts() {
    for remote in [false, true] {
        let (mut fixture, package) = start(remote).await;
        crate::approvals::install(&fixture.client, &["delegation"]).await;
        let repository = Repository::init(&fixture.root).unwrap();
        write(&fixture.root, "alpha/value.rs", "old_alpha();\n");
        write(&fixture.root, "beta/value.py", "old_beta()\n");
        commit(&repository, "Initial receipt groups");
        write(&fixture.root, "alpha/value.rs", "new_alpha();\n");
        write(&fixture.root, "beta/value.py", "new_beta()\n");
        let scope = scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
        let first = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "review_complete"),
            group_receipt(&scope, "alpha/value.rs"),
        )])
        .await;
        let second = Server::scripted_tools(vec![(
            plugin_tool(PACKAGE, "review_complete"),
            group_receipt(&scope, "beta/value.py"),
        )])
        .await;
        let profiles = roles(&mut fixture, &[("alpha", &first), ("beta", &second)]).await;
        permission(&mut fixture, Permission::Ask).await;
        let parent = Server::turn_parallel(vec![
            (
                plugin_tool("delegation", "spawn_agent"),
                json!({"role":"alpha","task":"Submit the complete alpha group receipt"}),
            ),
            (
                plugin_tool("delegation", "spawn_agent"),
                json!({"role":"beta","task":"Submit the complete beta group receipt"}),
            ),
        ])
        .await;
        route(&fixture.client, &fixture.session, &parent.endpoint).await;
        let turn = submit(&fixture.client, &fixture.session).await;
        let page = observe(&fixture.client, fixture.session.id, |page| {
            page.children
                .iter()
                .filter(|child| child.origin.turn == turn.id)
                .count()
                == 2
        })
        .await;
        let child = |role: RoleId| {
            page.children
                .iter()
                .find(|child| child.origin.turn == turn.id && child.origin.role == Some(role))
                .unwrap()
                .run
                .session
        };
        let alpha = child(profiles[0].id);
        let beta = child(profiles[1].id);
        let alpha_receipt = stage(&fixture, alpha, "review_complete").await;
        let beta_receipt = stage(&fixture, beta, "review_complete").await;
        let key = format!("review/{}/head", scope["review_id"].as_str().unwrap());
        let prepared = value(&fixture, &package, &key).await;
        assert!(
            prepared.value["reviewed_groups"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        tokio::join!(
            process::decide(&fixture.client, &alpha_receipt, Decision::Approve),
            process::decide(&fixture.client, &beta_receipt, Decision::Approve),
        );
        completed(&fixture.client, fixture.session.id, turn.id).await;
        let alpha_page = completed(&fixture.client, alpha, alpha_receipt.turn).await;
        let beta_page = completed(&fixture.client, beta, beta_receipt.turn).await;
        let alpha_results = crate::approvals::rules::results(&alpha_page, alpha_receipt.turn);
        let beta_results = crate::approvals::rules::results(&beta_page, beta_receipt.turn);
        assert_eq!(alpha_results.len(), 1);
        assert_eq!(beta_results.len(), 1);
        let winners = [won(&alpha_results[0].2), won(&beta_results[0].2)];
        assert!(winners.into_iter().any(|won| won));
        let children = [
            (alpha, "alpha/value.rs", alpha_receipt.turn),
            (beta, "beta/value.py", beta_receipt.turn),
        ];
        let findings = BTreeMap::new();
        let mut groups = BTreeMap::new();
        for (index, (child, path, _)) in children.iter().enumerate() {
            if winners[index] {
                assert!(groups.insert(group_id(&scope, path), *child).is_none());
            }
        }
        retained(&fixture, &package, &scope, &findings, &groups).await;
        for (index, (child, path, previous)) in children.iter().enumerate() {
            if winners[index] {
                continue;
            }
            let results = retry(
                &fixture,
                *child,
                *previous,
                if index == 0 { &first } else { &second },
                vec![(
                    plugin_tool(PACKAGE, "review_complete"),
                    group_receipt(&scope, path),
                )],
            )
            .await;
            assert_eq!(results[0]["status"], "open");
            assert!(groups.insert(group_id(&scope, path), *child).is_none());
        }
        assert_eq!(groups.len(), 2);
        retained(&fixture, &package, &scope, &findings, &groups).await;
        fixture = restart(fixture, remote).await;
        retained(&fixture, &package, &scope, &findings, &groups).await;
        permission(&mut fixture, Permission::Full).await;
        let result = call(&fixture, "review_complete", finalize(&scope)).await;
        success(&result);
        assert_eq!(result["status"], "complete");
        assert_eq!(result["findings"], 0);
        stop(fixture).await;
    }
}

#[tokio::test]
async fn rejects_foreign_sessions_and_worktrees() {
    for remote in [false, true] {
        let (mut fixture, package) = start(remote).await;
        crate::approvals::install(&fixture.client, &["delegation"]).await;
        let repository = Repository::init(&fixture.root).unwrap();
        write(&fixture.root, "src/value.rs", "old_value();\n");
        commit(&repository, "Initial scope");
        write(&fixture.root, "src/value.rs", "new_value();\n");
        let scope = scope(&fixture, json!({"kind":"worktree","data":{"scope":"all"}})).await;
        let key = format!("review/{}/head", scope["review_id"].as_str().unwrap());
        let original = value(&fixture, &package, &key).await;
        let Output::Session(foreign) = execute(
            &fixture.client,
            Command::CreateSession {
                project: fixture.session.project,
                worktree: Some(fixture.session.worktree),
                config: Some(fixture.session.config.clone()),
            },
        )
        .await
        else {
            panic!("session expected")
        };
        let request = json!({"review_id":scope["review_id"]});
        let (results, _) = calls_in(
            &fixture,
            &foreign,
            vec![
                (plugin_tool(PACKAGE, "review_scope"), request.clone()),
                (
                    plugin_tool(PACKAGE, "report_finding"),
                    finding(&scope, "src/value.rs", "new", 1, "new_value();"),
                ),
                (
                    plugin_tool(PACKAGE, "review_complete"),
                    receipts(&scope, true),
                ),
            ],
        )
        .await;
        for result in &results {
            failure(result, "permission_denied");
        }
        assert_eq!(value(&fixture, &package, &key).await, original);
        let linked = fixture.directory.path().join("linked");
        repository.worktree("linked", &linked, None).unwrap();
        let Output::Worktree(worktree) = execute(
            &fixture.client,
            Command::RegisterWorktree {
                project: fixture.session.project.unwrap(),
                path: linked.to_str().unwrap().into(),
            },
        )
        .await
        else {
            panic!("worktree expected")
        };
        let delegated = Server::turn_tools(vec![
            (plugin_tool(PACKAGE, "review_scope"), request.clone()),
            (
                plugin_tool(PACKAGE, "report_finding"),
                finding(&scope, "src/value.rs", "new", 1, "new_value();"),
            ),
            (
                plugin_tool(PACKAGE, "review_complete"),
                receipts(&scope, true),
            ),
        ])
        .await;
        roles(&mut fixture, &[("foreign-worktree", &delegated)]).await;
        let (delegation,_) = calls(&fixture,vec![(plugin_tool("delegation","spawn_agent"),
            json!({"role":"foreign-worktree","worktree":worktree.id,
                "task":"Try to read, report and finalize the captured review from this other worktree"}))]).await;
        assert_eq!(delegation[0]["status"], "completed", "{delegation:?}");
        let child: SessionId = delegation[0]["session"].as_str().unwrap().parse().unwrap();
        let page = history(&fixture.client, child).await;
        assert_eq!(page.runs.len(), 1);
        let results = crate::approvals::rules::results(&page, page.runs[0].turn);
        assert_eq!(results.len(), 3);
        for (_, _, result) in results {
            failure(&result, "permission_denied");
        }
        assert_eq!(value(&fixture, &package, &key).await, original);
        let Output::Session(moved) = execute(
            &fixture.client,
            Command::MoveConversation {
                session: fixture.session.id,
                worktree: worktree.id,
                expected_revision: fixture.session.revision,
            },
        )
        .await
        else {
            panic!("session expected")
        };
        fixture.session = moved;
        failure(
            &call(&fixture, "review_scope", request).await,
            "permission_denied",
        );
        assert_eq!(value(&fixture, &package, &key).await, original);
        stop(fixture).await;
    }
}
