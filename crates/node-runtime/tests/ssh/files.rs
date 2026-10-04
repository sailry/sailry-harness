use super::*;
use sailry_link::CancellationToken;

#[tokio::test]
async fn browses_and_streams_files() {
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("ssh");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("folder")).unwrap();
        let server = server::Server::start(root.clone(), 52).await;
        let node = Node::start(directory.path().join("node")).await.unwrap();
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
        let profile = save(
            &client,
            profile(server.port),
            Some(Credential::Password {
                password: Secret::new("isolated-ssh-password".into()),
            }),
        )
        .await;
        let browse = |profile: &Profile, path: &str| Command::BrowseSshDirectory {
            profile: profile.id,
            expected_revision: profile.revision,
            path: path.into(),
            after: None,
        };
        assert!(matches!(
            execute(&client, browse(&profile, ".")).await,
            Output::SshOutcome(Outcome::HostKeyRequired { .. })
        ));
        let profile = trust(&client, &profile, &server.key).await;
        let Output::SshOutcome(Outcome::Directory(list)) =
            execute(&client, browse(&profile, ".")).await
        else {
            panic!("directory expected");
        };
        assert_eq!(list.path, root.canonicalize().unwrap().to_str().unwrap());
        assert!(
            list.entries
                .iter()
                .any(|entry| entry.name == "folder" && entry.kind == EntryKind::Directory)
        );
        let name = "文件 ' $(touch unexpected).bin";
        let path = format!("{}/{name}", list.path);
        let data: Vec<u8> = (0..2 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let spec = ssh::UploadSpec {
            profile: profile.id,
            expected_revision: profile.revision,
            path: path.clone(),
            size: data.len() as u64,
            revision: blake3::hash(&data).to_hex().to_string(),
            overwrite: false,
        };
        let Output::SshUpload(upload) =
            execute(&client, Command::StageSshUpload(spec.clone())).await
        else {
            panic!("upload expected");
        };
        client
            .upload_ssh(
                &upload,
                &mut data.as_slice(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        let wrong = client.prepare(Command::FinishSshUpload {
            profile: profile.id,
            expected_revision: profile.revision,
            path: format!("{}.other", path),
            stream: upload.stream,
        });
        assert!(client.execute(wrong).await.is_err());
        let finish = client.prepare(Command::FinishSshUpload {
            profile: profile.id,
            expected_revision: profile.revision,
            path: path.clone(),
            stream: upload.stream,
        });
        assert!(finish.command.durable());
        assert_eq!(
            client.execute(finish.clone()).await.unwrap(),
            Output::SshOutcome(Outcome::Transferred {
                bytes: data.len() as u64
            })
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), data);
        assert!(!root.join("unexpected").exists());
        std::fs::write(root.join(name), "changed").unwrap();
        client.execute(finish).await.unwrap();
        assert_eq!(std::fs::read(root.join(name)).unwrap(), b"changed");
        let Output::SshUpload(upload) = execute(&client, Command::StageSshUpload(spec)).await
        else {
            panic!("upload expected");
        };
        client
            .upload_ssh(
                &upload,
                &mut data.as_slice(),
                CancellationToken::new(),
                |_| {},
            )
            .await
            .unwrap();
        let finish = client.prepare(Command::FinishSshUpload {
            profile: profile.id,
            expected_revision: profile.revision,
            path: path.clone(),
            stream: upload.stream,
        });
        assert_eq!(
            client.execute(finish).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        assert_eq!(std::fs::read(root.join(name)).unwrap(), b"changed");
        std::fs::write(root.join(name), &data).unwrap();
        let Output::SshOutcome(Outcome::Download(download)) = execute(
            &client,
            Command::DownloadSshFile {
                profile: profile.id,
                expected_revision: profile.revision,
                path: path.clone(),
            },
        )
        .await
        else {
            panic!("download expected");
        };
        let mut received = Vec::new();
        client
            .download_ssh(&download, &mut received, CancellationToken::new(), |_| {})
            .await
            .unwrap();
        assert_eq!(received, data);
        assert!(
            client
                .download_ssh(&download, &mut Vec::new(), CancellationToken::new(), |_| {})
                .await
                .is_err()
        );
        assert_eq!(
            client
                .execute(client.prepare(browse(&profile, "/sailry-nonexistent-fixture")))
                .await
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        let paged = root.join("paged");
        std::fs::create_dir(&paged).unwrap();
        for index in 0..501 {
            std::fs::write(paged.join(format!("file-{index:04}")), []).unwrap();
        }
        let Output::SshOutcome(Outcome::Directory(first)) =
            execute(&client, browse(&profile, paged.to_str().unwrap())).await
        else {
            panic!("directory expected");
        };
        assert_eq!(first.entries.len(), MAX_DIRECTORY_ENTRIES);
        assert!(first.next.is_some());
        let Output::SshOutcome(Outcome::Directory(last)) = execute(
            &client,
            Command::BrowseSshDirectory {
                profile: profile.id,
                expected_revision: profile.revision,
                path: first.path,
                after: first.next,
            },
        )
        .await
        else {
            panic!("directory expected");
        };
        assert_eq!(last.entries.len(), 1);
        assert_eq!(last.entries[0].name, "file-0500");
        assert!(last.next.is_none());
        let modify = |path: &str, action| {
            client.prepare(Command::ModifySshFile {
                profile: profile.id,
                expected_revision: profile.revision,
                path: path.into(),
                action,
            })
        };
        use ssh::FileAction;
        let create = modify("operations", FileAction::Create { directory: true });
        client.execute(create.clone()).await.unwrap();
        client.execute(create).await.unwrap();
        client
            .execute(modify(
                "operations/nested",
                FileAction::Create { directory: true },
            ))
            .await
            .unwrap();
        client
            .execute(modify(
                "operations/nested/empty",
                FileAction::Create { directory: false },
            ))
            .await
            .unwrap();
        std::fs::write(root.join("operations/nested/value"), b"preserved").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("folder"), root.join("operations/link")).unwrap();
        client
            .execute(modify(
                "operations",
                FileAction::Copy {
                    destination: "copied".into(),
                },
            ))
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(root.join("copied/nested/value")).unwrap(),
            b"preserved"
        );
        assert_eq!(
            std::fs::read(root.join("copied/nested/empty")).unwrap(),
            b""
        );
        #[cfg(unix)]
        assert_eq!(
            std::fs::read_link(root.join("copied/link")).unwrap(),
            root.join("folder")
        );
        client
            .execute(modify(
                "copied",
                FileAction::Move {
                    destination: "moved ' 目录".into(),
                },
            ))
            .await
            .unwrap();
        assert!(!root.join("copied").exists());
        assert_eq!(
            client
                .execute(modify(
                    "operations",
                    FileAction::Copy {
                        destination: "operations/nested/copy".into()
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            client
                .execute(modify(
                    "operations",
                    FileAction::Move {
                        destination: "moved ' 目录".into()
                    }
                ))
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            client
                .execute(modify("/", FileAction::Remove))
                .await
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
        client
            .execute(modify("moved ' 目录", FileAction::Remove))
            .await
            .unwrap();
        assert!(!root.join("moved ' 目录").exists());
        assert!(root.join("folder").is_dir());
        assert!(root.join("operations/nested/value").is_file());
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
        server.close().await;
    }
}
