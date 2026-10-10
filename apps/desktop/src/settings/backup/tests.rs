use super::super::knowledge::tests::fixture::{Fixture, draw, init, tap, toast};
use super::*;
use core::prelude::v1::test;
use sailry_protocol::{
    Output, Secret,
    backup::{Kind, Update},
    plugin::settings::SecretUpdate,
};
use std::time::{Duration, Instant};

#[path = "../../../../../crates/node-runtime/src/backup/tests/server.rs"]
mod server;

fn selected(owner: &Entity<Workspace>, cx: &App) -> Entity<Panel> {
    let workspace = owner.read(cx);
    workspace
        .backup
        .panels
        .get(&workspace.backup.selected.unwrap())
        .unwrap()
        .clone()
}

#[track_caller]
fn wait(visual: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        visual.executor().advance_clock(Duration::from_millis(10));
        draw(visual);
        if visual.update(|_, cx| ready(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "backup UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn open(owner: &Entity<Workspace>, visual: &mut VisualTestContext) -> Entity<Panel> {
    visual.update(|_, cx| {
        owner.update(cx, |workspace, cx| {
            workspace.select(super::super::Section::Backup, cx)
        })
    });
    wait(visual, |cx| {
        let workspace = owner.read(cx);
        workspace
            .backup
            .selected
            .and_then(|node| workspace.backup.panels.get(&node))
            .is_some_and(|panel| panel.read(cx).settings.is_some() && !panel.read(cx).loading)
    });
    visual.update(|_, cx| selected(owner, cx))
}

fn settings(fixture: &Fixture) -> Settings {
    let Output::BackupSettings(settings) = fixture.execute(Command::ReadBackupSettings) else {
        panic!("backup settings expected");
    };
    settings
}

#[gpui::test]
fn autosaves_private_destination_on_selected_node(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let server = fixture
            .runtime
            .block_on(server::Server::start(Kind::Webdav));
        let (owner, visual) = fixture.mount(cx);
        let panel = open(&owner, visual);
        tap(visual, "backup-configure");
        let editor =
            visual.update(|_, cx| panel.read(cx).editor.as_ref().unwrap().upgrade().unwrap());
        visual.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                for (index, value) in [
                    (0, server.endpoint.as_str()),
                    (1, "profiles"),
                    (4, "fixture-user"),
                    (5, "fixture-password"),
                    (7, "fixture-backup-passphrase"),
                ] {
                    editor.fields[index].1.update(cx, |input, cx| {
                        input.set_value(value, window, cx);
                        cx.emit(input::InputEvent::Change);
                    });
                }
            })
        });
        wait(visual, |cx| {
            panel
                .read(cx)
                .settings
                .as_ref()
                .is_some_and(|settings| settings.revision == 1)
                && !panel.read(cx).pending
        });
        assert!(settings(&fixture).credentials_configured);
        assert!(settings(&fixture).passphrase_configured);
        assert_eq!(
            settings(&fixture).destination.unwrap().endpoint,
            server.endpoint
        );
        visual.update(|_, cx| {
            for (_, input) in &editor.read(cx).fields[4..] {
                assert!(input.read(cx).value().is_empty());
            }
        });
        assert!(
            !serde_json::to_string(&settings(&fixture))
                .unwrap()
                .contains("fixture-password")
        );
        assert!(
            !serde_json::to_string(&settings(&fixture))
                .unwrap()
                .contains("fixture-backup-passphrase")
        );
        let other = sailry_client::Client::new(fixture.other.local());
        let Output::BackupSettings(other_settings) = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadBackupSettings)))
            .unwrap()
        else {
            panic!("backup settings expected");
        };
        assert_eq!(other_settings, Settings::default());
        draw(visual);
        assert!(visual.debug_bounds("backup-editor-save").is_none());
        tap(visual, "backup-editor-close");
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(server.writes(), 0);
        fixture.close(visual);
    }
}

#[gpui::test]
fn exports_and_restores_through_native_controls(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let server = fixture.runtime.block_on(server::Server::start(if remote {
            Kind::S3
        } else {
            Kind::Webdav
        }));
        let mut saved = settings(&fixture);
        saved.destination = Some(server.destination());
        fixture.execute(Command::SaveBackupSettings(Update {
            settings: saved,
            identity: SecretUpdate::Replace(Secret::new("fixture-user".into())),
            secret: SecretUpdate::Replace(Secret::new("fixture-password".into())),
            session_token: SecretUpdate::Keep,
            passphrase: SecretUpdate::Replace(Secret::new("fixture-backup-passphrase".into())),
        }));
        let (owner, visual) = fixture.mount(cx);
        let panel = open(&owner, visual);
        tap(visual, "backup-export");
        wait(visual, |cx| {
            panel
                .read(cx)
                .page
                .as_ref()
                .is_some_and(|page| page.records.len() == 1)
                && !panel.read(cx).pending
                && !panel.read(cx).loading
        });
        let record =
            visual.update(|_, cx| panel.read(cx).page.as_ref().unwrap().records[0].clone());
        assert!(server.parts(&record.id) > 0);
        assert!(server.maximum_put() <= 4 * 1024 * 1024);
        let original = fixture.directory.path().join("node/storage/node.sqlite3");
        assert!(original.is_file());
        let parent = fixture.directory.path().canonicalize().unwrap();
        let target = parent.join("restored");
        tap(visual, &format!("backup-restore-{}", record.id));
        let form =
            visual.update(|_, cx| panel.read(cx).restore.as_ref().unwrap().upgrade().unwrap());
        visual.update(|window, cx| {
            form.read(cx).target.clone().update(cx, |input, cx| {
                input.set_value(target.to_str().unwrap(), window, cx)
            })
        });
        assert!(!target.exists());
        tap(visual, "backup-restore-confirm");
        wait(visual, |cx| {
            !panel.read(cx).pending && panel.read(cx).request.is_none()
        });
        assert!(target.join("storage/node.sqlite3").is_file());
        assert!(original.is_file());
        assert!(!target.join("node.lock").exists());
        toast(visual, "backup_restored");
        server.corrupt(&record.id);
        let corrupt = parent.join("corrupt");
        tap(visual, &format!("backup-restore-{}", record.id));
        let form =
            visual.update(|_, cx| panel.read(cx).restore.as_ref().unwrap().upgrade().unwrap());
        visual.update(|window, cx| {
            form.read(cx).target.clone().update(cx, |input, cx| {
                input.set_value(corrupt.to_str().unwrap(), window, cx)
            })
        });
        tap(visual, "backup-restore-confirm");
        wait(visual, |cx| {
            panel.read(cx).error.is_some() && !panel.read(cx).pending
        });
        assert!(!corrupt.exists());
        assert_eq!(
            visual.update(|_, cx| panel.read(cx).page.as_ref().unwrap().records.len()),
            1
        );
        fixture.close(visual);
    }
}

#[gpui::test]
fn preserves_conflicts_and_closes_stale_editors(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        let server = fixture
            .runtime
            .block_on(server::Server::start(Kind::Webdav));
        let mut saved = settings(&fixture);
        saved.destination = Some(server.destination());
        fixture.execute(Command::SaveBackupSettings(Update {
            settings: saved,
            identity: SecretUpdate::Keep,
            secret: SecretUpdate::Keep,
            session_token: SecretUpdate::Keep,
            passphrase: SecretUpdate::Replace(Secret::new("fixture-backup-passphrase".into())),
        }));
        let (owner, visual) = fixture.mount(cx);
        let panel = open(&owner, visual);
        tap(visual, "backup-configure");
        let editor =
            visual.update(|_, cx| panel.read(cx).editor.as_ref().unwrap().upgrade().unwrap());
        visual.update(|window, cx| {
            editor.read(cx).fields[1]
                .1
                .clone()
                .update(cx, |input, cx| input.set_value("draft-prefix", window, cx))
        });
        let mut current = settings(&fixture);
        current.destination.as_mut().unwrap().root = "external-prefix".into();
        fixture.execute(Command::SaveBackupSettings(Update {
            settings: current,
            identity: SecretUpdate::Keep,
            secret: SecretUpdate::Keep,
            session_token: SecretUpdate::Keep,
            passphrase: SecretUpdate::Replace(Secret::new("fixture-backup-passphrase".into())),
        }));
        wait(visual, |cx| editor.read(cx).conflict);
        assert_eq!(
            visual.update(|_, cx| editor.read(cx).fields[1].1.read(cx).value().to_string()),
            "draft-prefix"
        );
        assert_eq!(
            settings(&fixture).destination.unwrap().root,
            "external-prefix"
        );
        visual.update(|_, cx| {
            owner.update(cx, |workspace, cx| {
                workspace.bind_providers(
                    fixture.other.local(),
                    fixture.runtime.clone(),
                    "Other host".into(),
                    cx,
                )
            })
        });
        wait(visual, |cx| !panel.read(cx).active);
        tap(visual, "backup-editor-close");
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert_eq!(settings(&fixture).revision, 2);
        let other = sailry_client::Client::new(fixture.other.local());
        let Output::BackupSettings(other_settings) = fixture
            .runtime
            .block_on(other.execute(other.prepare(Command::ReadBackupSettings)))
            .unwrap()
        else {
            panic!("backup settings expected");
        };
        assert_eq!(other_settings, Settings::default());
        fixture.close(visual);
    }
}
