use super::*;
use crate::plugins::fixture::Fixture;
use sailry_protocol::{Command, Output};
use std::sync::atomic::Ordering;

fn package(fixture: &Fixture, name: &str) -> Summary {
    let Output::Plugin(info) = fixture.execute(Command::ReadPlugin { name: name.into() }) else {
        panic!("plugin expected");
    };
    info.summary
}

fn resources(fixture: &Fixture) -> Resources {
    Resources::new(
        fixture.binding.client.clone(),
        fixture.runtime.handle().clone(),
    )
}

#[test]
fn reads_and_invalidation() {
    for (scenario, check) in [
        (
            "coalesces_reads_without_running_scripts",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                let package = package(&fixture, "media").reference();
                *fixture.transport.view_target.lock().unwrap() = Some(package.clone());
                let mut resources = resources(&fixture);
                let first = resources.prepare(package.clone());
                let second = resources.prepare(package);
                let (first, second) = fixture
                    .runtime
                    .block_on(async { futures::join!(first.load(), second.load()) });
                let first = first.unwrap().unwrap();
                let second = second.unwrap().unwrap();
                assert!(
                    Arc::ptr_eq(&first, &second),
                    "coalesces reads without running scripts"
                );
                assert_eq!(
                    fixture.transport.views.lock().unwrap().len(),
                    1,
                    "coalesces reads without running scripts"
                );
                assert!(
                    fixture.transport.requests.lock().unwrap().is_empty(),
                    "coalesces reads without running scripts: preloading must not run plugin initialization"
                );
                let root = first.root().to_owned();
                drop(resources);
                assert!(
                    root.exists(),
                    "coalesces reads without running scripts: a mounted consumer owns its resource files"
                );
                drop(first);
                drop(second);
                fixture.runtime.block_on(async {
                    tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        while root.exists() {
                            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        }
                    })
                    .await
                    .unwrap();
                });
                fixture.close();
            }) as fn(&str),
        ),
        (
            "retries_failed_preloads",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                let package = package(&fixture, "media").reference();
                *fixture.transport.view_target.lock().unwrap() = Some(package.clone());
                fixture.transport.mode.store(16, Ordering::SeqCst);
                let mut resources = resources(&fixture);
                assert!(
                    fixture
                        .runtime
                        .block_on(resources.prepare(package.clone()).load())
                        .unwrap()
                        .is_err(),
                    "retries failed preloads"
                );
                assert_eq!(
                    fixture.transport.views.lock().unwrap().len(),
                    3,
                    "retries failed preloads"
                );
                fixture.transport.mode.store(0, Ordering::SeqCst);
                assert!(
                    fixture
                        .runtime
                        .block_on(resources.prepare(package).load())
                        .unwrap()
                        .is_ok(),
                    "retries failed preloads"
                );
                assert_eq!(
                    fixture.transport.views.lock().unwrap().len(),
                    4,
                    "retries failed preloads"
                );
                drop(resources);
                fixture.close();
            }),
        ),
        (
            "bounds_resources_and_invalidates_revisions",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                fixture.package();
                let path = fixture.directory.path().join("project/package/plugin.json");
                let mut manifest: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
                let extension = &mut manifest["extensions"]["dev.sailry.platform"];
                extension["settings_page"]["entry"] = extension["desktop"]["entry"].clone();
                std::fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
                fixture.install(0);
                let mut resources = resources(&fixture);
                let mut summaries = Vec::new();
                for name in [
                    "browser",
                    "web-search",
                    "project-summary",
                    "media",
                    "memory",
                ] {
                    let summary = package(&fixture, name);
                    let prepared = fixture
                        .runtime
                        .block_on(resources.prepare(summary.reference()).load())
                        .unwrap();
                    assert!(
                        prepared.is_ok(),
                        "bounds resources and invalidates revisions: {name}: {:?}",
                        prepared.err()
                    );
                    summaries.push(summary);
                }
                assert_eq!(
                    resources.entries.len(),
                    CAPACITY,
                    "bounds resources and invalidates revisions"
                );
                assert!(
                    !resources
                        .entries
                        .iter()
                        .any(|entry| entry.prepared.package.name == "browser"),
                    "bounds resources and invalidates revisions"
                );
                summaries.last_mut().unwrap().settings_revision += 1;
                resources.retain(&summaries);
                assert!(
                    !resources
                        .entries
                        .iter()
                        .any(|entry| entry.prepared.package.name == "memory"),
                    "bounds resources and invalidates revisions"
                );
                resources.retain(&[]);
                assert!(
                    resources.entries.is_empty(),
                    "bounds resources and invalidates revisions"
                );
                drop(resources);
                fixture.close();
            }),
        ),
        (
            "dropping_resources_cancels_pending_reads",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                let package = package(&fixture, "media").reference();
                *fixture.transport.view_target.lock().unwrap() = Some(package.clone());
                fixture.transport.mode.store(16, Ordering::SeqCst);
                let mut resources = resources(&fixture);
                let pending = resources.prepare(package);
                fixture.runtime.block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        fixture.transport.entered.cancelled(),
                    )
                    .await
                    .unwrap();
                });
                drop(resources);
                assert!(
                    fixture.runtime.block_on(pending.load()).is_none(),
                    "dropping resources cancels pending reads"
                );
                fixture.close();
            }),
        ),
        (
            "recovers_transient_reads_without_reopening",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                let package = package(&fixture, "media").reference();
                *fixture.transport.view_target.lock().unwrap() = Some(package.clone());
                fixture.transport.mode.store(18, Ordering::SeqCst);
                let mut resources = resources(&fixture);
                assert!(
                    fixture
                        .runtime
                        .block_on(resources.prepare(package).load())
                        .unwrap()
                        .is_ok(),
                    "recovers transient reads without reopening"
                );
                assert_eq!(
                    fixture.transport.views.lock().unwrap().len(),
                    2,
                    "recovers transient reads without reopening"
                );
                drop(resources);
                fixture.close();
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
