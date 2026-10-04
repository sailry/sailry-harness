use super::*;

async fn setup(client: &Client, context: &Context) {
    dispatch::run(
        client,
        context,
        jobs::Command::SaveHandler(jobs::Handler {
            name: "receive".into(),
            revision: 0,
            enabled: true,
            source: jobs::Source {
                package: "example".into(),
                topic: "tick".into(),
            },
            queue: "versions".into(),
            callback: jobs::Callback {
                scope: jobs::Scope {
                    worktree: context.worktree,
                    session: None,
                },
                completion: jobs::Completion::Command,
                command: Box::new(Command::CallPlugin {
                    handler: "run".into(),
                    input: json!(null),
                }),
                bindings: std::collections::BTreeMap::from([(
                    "/data/input".into(),
                    "/payload".into(),
                )]),
            },
        }),
    )
    .await;
}

#[tokio::test]
async fn freezes_running_versions_and_updates_future_deliveries() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = fixture(remote).await;
        let root = directory.path().join("source/package");
        let code = |version: &str| {
            format!(
                r#"
            import {{getValue, readSettings, setValue, completeRequest}} from 'sailry/sdk';
            export async function run(input) {{
                if (input.wait) {{
                    await completeRequest(setValue('started', true, '0'));
                    while (!(await getValue('release')).value) {{}}
                }}
                const settings = await readSettings();
                const result = await completeRequest(setValue(input.key, '{version}-' + settings.values.label, '0'));
                if (result.Err) throw new Error('Write failed');
                return result.Ok.data;
            }}
        "#
            )
        };
        let settings = |label: &str| json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"properties":{"label":{"type":"string","default":label}}});
        fs::write(
            root.join("dev.sailry.platform/settings.json"),
            settings("first").to_string(),
        )
        .unwrap();
        let old = installed(&client, &root, worktree, &code("old"), 0).await;
        setup(&client, &old).await;
        dispatch::run(
            &client,
            &old,
            jobs::Command::Enqueue {
                handler: "receive".into(),
                payload: json!({"key":"before","wait":true}),
            },
        )
        .await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while !read(&client, &old, "started").await.present {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        fs::write(
            root.join("dev.sailry.platform/settings.json"),
            settings("second").to_string(),
        )
        .unwrap();
        let current = installed(&client, &root, worktree, &code("new"), 1).await;
        assert_ne!(old.package.digest, current.package.digest);
        dispatch::run(
            &client,
            &current,
            jobs::Command::Enqueue {
                handler: "receive".into(),
                payload: json!({"key":"enqueued"}),
            },
        )
        .await;
        dispatch::run(
            &client,
            &current,
            jobs::Command::Publish {
                topic: "tick".into(),
                payload: json!({"key":"published"}),
            },
        )
        .await;
        assert!(
            directory
                .path()
                .join("node/plugins/packages")
                .join(&old.package.digest)
                .exists()
        );
        assert_eq!(
            client
                .execute(call(&client, &old, json!({"key":"forged"})))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
        client
            .execute(
                client
                    .prepare(Command::WritePluginValue {
                        key: "release".into(),
                        value: json!(true),
                        expected_revision: 0,
                    })
                    .with_plugin(current.clone()),
            )
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let jobs::Output::Jobs(page) = dispatch::run(
                    &client,
                    &current,
                    jobs::Command::ListJobs {
                        before: None,
                        limit: 10,
                    },
                )
                .await
                else {
                    panic!("jobs expected")
                };
                assert_eq!(page.jobs.len(), 3);
                if page
                    .jobs
                    .iter()
                    .all(|job| !matches!(job.status, jobs::Status::Queued | jobs::Status::Running))
                {
                    assert!(
                        page.jobs
                            .iter()
                            .all(|job| job.status == jobs::Status::Completed),
                        "{:?}",
                        page.jobs
                    );
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        for (key, expected) in [
            ("before", "old-first"),
            ("enqueued", "new-second"),
            ("published", "new-second"),
        ] {
            assert_eq!(read(&client, &current, key).await.value, json!(expected));
        }
        assert!(!read(&client, &current, "forged").await.present);
        node.shutdown().await.unwrap();
        assert!(
            !directory
                .path()
                .join("node/plugins/packages")
                .join(&old.package.digest)
                .exists()
        );
        controller.close().await.unwrap();
    }
}
