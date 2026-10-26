use super::*;
use serde_json::json;

fn manifest(version: &str, minimum: &str) -> serde_json::Value {
    json!({
        "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name":"official-new", "version":version,
        "extensions":{"dev.sailry.platform":{"api_version":"v1", "min_sailry_version":minimum}}
    })
}

fn index() -> serde_json::Value {
    let mut value = json!({"version":1, "skills":[], "packages":[{
        "id":"official-new", "manifest":manifest("2.0.0", "99.0.0"),
        "release_notes":{"text":"Future notes"},
        "versions":[
            {"version":"1.0.0", "commit":FIRST, "min_sailry_version":"0.1.0-alpha.4", "release_notes":{"text":"Compatible notes"}},
            {"version":"2.0.0", "commit":SECOND, "min_sailry_version":"99.0.0", "release_notes":{"text":"Future notes"}}
        ]
    }]});
    value["packages"][0]["manifest"]["extensions"]["dev.sailry.platform"]["actions"] =
        json!(["files.read"]);
    value
}

#[tokio::test]
async fn selects_compatible_immutable_versions() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        catalog.updated.store(true, Ordering::SeqCst);
        let old = manifest("1.0.0", "0.1.0-alpha.4");
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        use std::io::Write;
        zip.start_file(
            "repo-source/official-new/plugin.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&serde_json::to_vec(&old).unwrap()).unwrap();
        zip.start_file(
            "repo-source/official-new/CHANGELOG.md",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"# Changelog\n\n## 1.0.0\n\nCompatible notes\n")
            .unwrap();
        let archive = zip.finish().unwrap().into_inner();
        {
            let mut documents = catalog.documents.lock().unwrap();
            documents.insert(
                format!("/{SECOND}/catalog.json"),
                serde_json::to_vec(&index()).unwrap(),
            );
            documents.insert(
                format!("/{FIRST}/official-new/plugin.json"),
                serde_json::to_vec(&old).unwrap(),
            );
            documents.insert(format!("/sailry/sailry-plugins/zip/{FIRST}"), archive);
        }
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let Output::PluginCatalog(page) = execute(&client, search("official-new")).await else {
            panic!("catalog expected");
        };
        assert_eq!(page.entries.len(), 1);
        let entry = &page.entries[0];
        assert_eq!(entry.version.as_deref(), Some("1.0.0"));
        assert_eq!(entry.commit.as_deref(), Some(FIRST));
        assert!(entry.compatibility.is_compatible());
        assert!(entry.permissions.is_empty());
        assert_eq!(
            entry.release_notes.as_ref().unwrap().text,
            "Compatible notes"
        );
        let inspected = details(
            execute(
                &client,
                Command::ReadCatalogPluginInfo {
                    source: plugin::catalog::Source::Official,
                    id: entry.id.clone(),
                    bundled: false,
                },
            )
            .await,
        );
        assert_eq!(inspected.versions.len(), 2);
        assert_eq!(inspected.versions[0].version, "2.0.0");
        assert_eq!(
            inspected.versions[0].compatibility.required(),
            Some("99.0.0")
        );
        let Output::PluginRepository(source) = execute(
            &client,
            Command::ReadCatalogPlugin {
                source: plugin::catalog::Source::Official,
                id: entry.id.clone(),
            },
        )
        .await
        else {
            panic!("repository expected");
        };
        assert_eq!(source.git_ref.as_deref(), Some(FIRST));
        let Output::PluginSource(selected) =
            execute(&client, Command::InspectPluginSource { source }).await
        else {
            panic!("selection expected");
        };
        let installed = info(
            execute(
                &client,
                Command::InstallPluginSource {
                    source: selected.source,
                    path: selected.path,
                    name: entry.id.clone(),
                    expected_revision: 0,
                },
            )
            .await,
        );
        let Output::PluginUpdates(batch) = execute(
            &client,
            Command::CheckPluginUpdates {
                force: false,
                packages: vec![plugin::updates::Request {
                    name: entry.id.clone(),
                    expected_revision: installed.summary.revision,
                }],
            },
        )
        .await
        else {
            panic!("updates expected");
        };
        assert!(batch.failures.is_empty());
        assert!(batch.reports[0].available.is_none());
        let blocked = batch.reports[0].blocked.as_ref().unwrap();
        assert_eq!(blocked.version, "2.0.0");
        assert_eq!(blocked.required, "99.0.0");
        assert_eq!(blocked.release_notes.as_ref().unwrap().text, "Future notes");
        assert_eq!(installed.summary.version.as_deref(), Some("1.0.0"));
        assert_eq!(
            installed.release_notes.as_ref().unwrap().text,
            "Compatible notes"
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn skips_invalid_packages_and_blocks_future_sources() {
    for remote in [false, true] {
        let catalog = Catalog::start().await;
        let value = json!({"version":1,"skills":[],"packages":[
            {"id":"bad", "manifest":{"name":"bad"}},
            {"id":"official-new", "manifest":manifest("2.0.0", "99.0.0")}
        ]});
        catalog.documents.lock().unwrap().insert(
            format!("/{FIRST}/catalog.json"),
            serde_json::to_vec(&value).unwrap(),
        );
        let (_directory, node, controller, client) = connection(remote, &catalog).await;
        let Output::PluginCatalog(page) = execute(&client, search("official-new")).await else {
            panic!("catalog expected");
        };
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.entries[0].compatibility.required(), Some("99.0.0"));
        let error = client
            .execute(client.prepare(Command::ReadCatalogPlugin {
                source: plugin::catalog::Source::Official,
                id: "official-new".into(),
            }))
            .await
            .unwrap_err();
        assert_eq!(
            error.details,
            Some(FaultDetails::PluginRequiresNewer {
                required: "99.0.0".into()
            })
        );
        controller.close().await.unwrap();
        node.shutdown().await.unwrap();
    }
}
