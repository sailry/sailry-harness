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
fn coalesces_reads_without_running_scripts() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
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
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 1);
        assert!(
            fixture.transport.requests.lock().unwrap().is_empty(),
            "preloading must not run plugin initialization"
        );
        let root = first.root().to_owned();
        drop(resources);
        assert!(root.exists(), "a mounted consumer owns its resource files");
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
    }
}

#[test]
fn retries_failed_preloads() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = package(&fixture, "media").reference();
        *fixture.transport.view_target.lock().unwrap() = Some(package.clone());
        fixture.transport.mode.store(16, Ordering::SeqCst);
        let mut resources = resources(&fixture);
        assert!(
            fixture
                .runtime
                .block_on(resources.prepare(package.clone()).load())
                .unwrap()
                .is_err()
        );
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 3);
        fixture.transport.mode.store(0, Ordering::SeqCst);
        assert!(
            fixture
                .runtime
                .block_on(resources.prepare(package).load())
                .unwrap()
                .is_ok()
        );
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 4);
        drop(resources);
        fixture.close();
    }
}

#[test]
fn bounds_resources_and_invalidates_revisions() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let mut resources = resources(&fixture);
        let mut summaries = Vec::new();
        for name in ["browser", "web-search", "delegation", "media", "memory"] {
            let summary = package(&fixture, name);
            let prepared = fixture
                .runtime
                .block_on(resources.prepare(summary.reference()).load())
                .unwrap();
            assert!(prepared.is_ok(), "{name}: {:?}", prepared.err());
            summaries.push(summary);
        }
        assert_eq!(resources.entries.len(), CAPACITY);
        assert!(
            !resources
                .entries
                .iter()
                .any(|entry| entry.prepared.package.name == "browser")
        );
        summaries.last_mut().unwrap().settings_revision += 1;
        resources.retain(&summaries);
        assert!(
            !resources
                .entries
                .iter()
                .any(|entry| entry.prepared.package.name == "memory")
        );
        resources.retain(&[]);
        assert!(resources.entries.is_empty());
        drop(resources);
        fixture.close();
    }
}

#[test]
fn dropping_resources_cancels_pending_reads() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
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
        assert!(fixture.runtime.block_on(pending.load()).is_none());
        fixture.close();
    }
}

#[test]
fn recovers_transient_reads_without_reopening() {
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let package = package(&fixture, "media").reference();
        *fixture.transport.view_target.lock().unwrap() = Some(package.clone());
        fixture.transport.mode.store(18, Ordering::SeqCst);
        let mut resources = resources(&fixture);
        assert!(
            fixture
                .runtime
                .block_on(resources.prepare(package).load())
                .unwrap()
                .is_ok()
        );
        assert_eq!(fixture.transport.views.lock().unwrap().len(), 2);
        drop(resources);
        fixture.close();
    }
}
