use super::*;
use sailry_link::CancellationToken;
use sailry_protocol::plugin::skills::{ArchiveFormat, ImportDiscovery, Imported};
use std::io::Write;

fn archive(format: ArchiveFormat) -> Vec<u8> {
    let body = "---\nname: analysis\ndescription: Inspect a sample\n---\nArchive body\n";
    if format == ArchiveFormat::Zip {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file(
            "analysis/SKILL.md",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(body.as_bytes()).unwrap();
        // A parent directory with multiple skills remains discoverable.
        zip.start_file("writing/SKILL.md", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"---\nname: writing\ndescription: Write a sample\n---\nWrite\n")
            .unwrap();
        return zip.finish().unwrap().into_inner();
    }
    let mut tar = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_mode(0o644);
    header.set_size(body.len() as u64);
    header.set_cksum();
    tar.append_data(&mut header, "analysis/SKILL.md", body.as_bytes())
        .unwrap();
    let bytes = tar.into_inner().unwrap();
    if format == ArchiveFormat::Tar {
        bytes
    } else {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&bytes).unwrap();
        encoder.finish().unwrap()
    }
}

async fn staged(client: &Client, bytes: &[u8], format: ArchiveFormat) -> ImportDiscovery {
    let Output::PluginUpload(upload) = execute(
        client,
        Command::UploadPlugin(plugin::UploadSpec {
            size: bytes.len() as u64,
            revision: blake3::hash(bytes).to_hex().to_string(),
        }),
    )
    .await
    else {
        panic!("upload expected");
    };
    client
        .upload(&upload, &mut &bytes[..], CancellationToken::new(), |_| {})
        .await
        .unwrap();
    let Output::SkillImportDiscovery(discovery) = execute(
        client,
        Command::DiscoverSkillUpload {
            stream: upload.stream,
            format,
            source: plugin::UploadSource::Archive,
        },
    )
    .await
    else {
        panic!("skill discovery expected");
    };
    assert_eq!(discovery.digest, upload.spec.revision);
    discovery
}

fn install(discovery: &ImportDiscovery, revision: u64) -> Command {
    let candidate = discovery
        .skills
        .iter()
        .find(|candidate| candidate.skill.name == "analysis")
        .unwrap();
    Command::InstallSkillUpload {
        stream: discovery.stream,
        source: Imported {
            digest: discovery.digest.clone(),
            path: candidate.path.clone(),
            kind: discovery.kind,
        },
        name: candidate.name.clone(),
        expected_revision: revision,
    }
}

#[tokio::test]
async fn freezes_local_directories_before_installing() {
    for remote in [false, true] {
        let (directory, node, controller, client, worktree) = support::empty(remote).await;
        let root = directory.path().join("source/imports/analysis");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("SKILL.md"),
            "---\nname: analysis\ndescription: Inspect a sample\n---\nOriginal body\n",
        )
        .unwrap();
        let Output::SkillImportDiscovery(discovery) = execute(
            &client,
            Command::DiscoverLocalSkills {
                worktree,
                path: "imports".into(),
            },
        )
        .await
        else {
            panic!("skill discovery expected");
        };
        assert_eq!(discovery.kind, plugin::UploadSource::Directory);
        fs::write(root.join("SKILL.md"), "Changed after discovery").unwrap();
        let request = client.prepare(install(&discovery, 0));
        let pending = client.dispatch(request.clone()).await.unwrap();
        assert!(pending.receipt.durable);
        let installed = info(pending.completion.await.unwrap().unwrap());
        assert_eq!(installed.summary.version, None);
        assert_eq!(installed.origin, Some(plugin::Origin::Directory));
        assert!(
            matches!(&installed.skill,Some(plugin::skills::Provenance::Imported{ imported }) if imported.digest == discovery.digest && imported.path == "analysis")
        );
        let body = fs::read_to_string(
            node.profile()
                .join("plugins/packages")
                .join(&installed.summary.digest)
                .join("skills/analysis/SKILL.md"),
        )
        .unwrap();
        assert!(body.ends_with("Original body\n"));
        assert_eq!(
            client.execute(request).await.unwrap(),
            Output::Plugin(installed.clone())
        );
        assert!(
            client
                .execute(client.prepare(install(&discovery, 1)))
                .await
                .is_err()
        );
        for path in ["../private", ".git/config", ".sailry/worktrees/another"] {
            assert!(
                client
                    .execute(client.prepare(Command::DiscoverLocalSkills {
                        worktree,
                        path: path.into()
                    }))
                    .await
                    .is_err()
            );
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn reimports_archives_without_update_urls() {
    for remote in [false, true] {
        let (_directory, node, controller, client, _worktree) = support::empty(remote).await;
        let mut revision = 0;
        let mut name = None;
        for format in [ArchiveFormat::Zip, ArchiveFormat::Tar, ArchiveFormat::TarGz] {
            let discovery = staged(&client, &archive(format), format).await;
            let installed = info(execute(&client, install(&discovery, revision)).await);
            if let Some(name) = &name {
                assert_eq!(&installed.summary.name, name);
            }
            name = Some(installed.summary.name.clone());
            assert_eq!(installed.origin, Some(plugin::Origin::Archive));
            assert_eq!(installed.summary.revision, revision + 1);
            assert_eq!(installed.summary.enabled, revision == 0);
            let Output::PluginUpdate(report) = execute(
                &client,
                Command::CheckPluginUpdate {
                    name: installed.summary.name.clone(),
                    expected_revision: installed.summary.revision,
                    force: false,
                },
            )
            .await
            else {
                panic!("update report expected");
            };
            assert!(report.manual && report.available.is_none());
            let disabled = info(
                execute(
                    &client,
                    Command::SetPluginEnabled {
                        name: installed.summary.name,
                        expected_revision: installed.summary.revision,
                        enabled: false,
                    },
                )
                .await,
            );
            revision = disabled.summary.revision;
        }
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
