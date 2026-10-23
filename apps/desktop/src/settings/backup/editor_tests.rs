use super::*;
use core::prelude::v1::test;

fn online(kind: Kind) -> Destination {
    Destination {
        kind,
        endpoint: "https://backup.example.test".into(),
        root: "archives".into(),
        bucket: "backups".into(),
        region: "us-east-1".into(),
    }
}

#[test]
fn selects_one_destination_without_erasing_configuration() {
    let original = Settings {
        destination: Some(online(Kind::S3)),
        local: Some(Local {
            directory: "/node/backups".into(),
            enabled: true,
        }),
        schedule: Some(Schedule {
            enabled: true,
            timezone: "Asia/Shanghai".into(),
            hour: 3,
            minute: 15,
        }),
        ..Settings::default()
    };
    for tab in 0..3 {
        let mut settings = original.clone();
        storage(&mut settings, tab, online(Kind::Webdav), "/node/backups").unwrap();
        assert_eq!(settings.online_enabled, tab < 2);
        assert_eq!(settings.local.as_ref().unwrap().enabled, tab == 2);
        assert_eq!(storage_tab(&settings), tab);
        assert_eq!(settings.schedule, original.schedule);
        assert_eq!(settings.local.as_ref().unwrap().directory, "/node/backups");
        assert_eq!(
            settings.destination.as_ref().unwrap().kind,
            if tab == 0 { Kind::Webdav } else { Kind::S3 }
        );
    }
    assert!(original.online_enabled);
    assert!(original.local.as_ref().unwrap().enabled);
}

#[test]
fn rejects_incomplete_selected_storage() {
    let original = Settings {
        destination: Some(online(Kind::S3)),
        ..Settings::default()
    };
    let mut settings = original.clone();
    assert_eq!(
        storage(&mut settings, 2, online(Kind::Webdav), ""),
        Err("backup_invalid_target")
    );
    assert_eq!(settings, original);
    let mut missing_bucket = online(Kind::S3);
    missing_bucket.bucket.clear();
    assert_eq!(
        storage(&mut settings, 1, missing_bucket, "/node/backups"),
        Err("backup_invalid")
    );
    assert_eq!(settings, original);
    let mut missing_endpoint = online(Kind::Webdav);
    missing_endpoint.endpoint.clear();
    assert_eq!(
        storage(&mut settings, 0, missing_endpoint, "/node/backups"),
        Err("backup_invalid")
    );
    assert_eq!(settings, original);
}

fn draft(settings: &Settings) -> Draft {
    Draft {
        tab: storage_tab(settings),
        destination: destination(settings),
        directory: settings
            .local
            .as_ref()
            .map(|local| local.directory.clone())
            .unwrap_or_default(),
        identity: String::new(),
        secret: String::new(),
        session_token: String::new(),
        passphrase: String::new(),
        confirmation: String::new(),
        schedule: settings.schedule.clone().unwrap_or(Schedule {
            enabled: false,
            timezone: "UTC".into(),
            hour: 3,
            minute: 0,
        }),
        remove_credentials: false,
    }
}

#[test]
fn preserves_untouched_defaults() {
    let settings = Settings::default();
    let draft = draft(&settings);
    for mode in [Mode::Schedule, Mode::Storage] {
        assert!(!draft.dirty(&settings, mode));
        let update = draft.update(&settings, mode).unwrap();
        assert_eq!(update.settings, settings);
        assert_eq!(update.identity, SecretUpdate::Keep);
        assert_eq!(update.secret, SecretUpdate::Keep);
        assert_eq!(update.passphrase, SecretUpdate::Keep);
    }
}

#[test]
fn schedule_saves_never_commit_storage_drafts() {
    let settings = Settings {
        revision: 8,
        destination: Some(online(Kind::S3)),
        local: Some(Local {
            directory: "/node/backups".into(),
            enabled: true,
        }),
        ..Settings::default()
    };
    let mut draft = draft(&settings);
    draft.tab = 2;
    draft.destination.endpoint.clear();
    draft.directory = "/unsaved/storage".into();
    draft.secret = "temporary credential".into();
    draft.passphrase = "invalid".into();
    draft.remove_credentials = true;
    draft.schedule.enabled = true;
    draft.schedule.hour = 6;
    let update = draft.update(&settings, Mode::Schedule).unwrap();
    assert_eq!(update.settings.revision, 8);
    assert_eq!(update.settings.destination, settings.destination);
    assert_eq!(update.settings.local, settings.local);
    assert!(update.settings.online_enabled);
    assert_eq!(update.settings.schedule, Some(draft.schedule.clone()));
    assert_eq!(update.identity, SecretUpdate::Keep);
    assert_eq!(update.secret, SecretUpdate::Keep);
    assert_eq!(update.session_token, SecretUpdate::Keep);
    assert_eq!(update.passphrase, SecretUpdate::Keep);
    assert_eq!(draft.directory, "/unsaved/storage");
}

#[test]
fn storage_saves_preserve_current_schedule() {
    let mut settings = Settings {
        revision: 8,
        destination: Some(online(Kind::S3)),
        ..Settings::default()
    };
    let mut draft = draft(&settings);
    draft.destination.endpoint = "https://changed.example.test".into();
    draft.identity = "access key".into();
    draft.schedule.enabled = true;
    draft.schedule.timezone.clear();
    assert!(draft.dirty(&settings, Mode::Storage));
    let update = draft.update(&settings, Mode::Storage).unwrap();
    assert_eq!(update.settings.revision, 8);
    assert_eq!(update.settings.schedule, None);
    assert_eq!(
        update.settings.destination.as_ref().unwrap().endpoint,
        "https://changed.example.test"
    );
    assert_eq!(
        update.identity,
        SecretUpdate::Replace(Secret::new("access key".into()))
    );
    settings.revision = 9;
    settings.schedule = Some(Schedule {
        enabled: true,
        timezone: "Asia/Shanghai".into(),
        hour: 7,
        minute: 30,
    });
    let rebased = draft.update(&settings, Mode::Storage).unwrap();
    assert_eq!(rebased.settings.revision, 9);
    assert_eq!(rebased.settings.schedule, settings.schedule);
    assert_eq!(rebased.identity, update.identity);
    assert_eq!(rebased.settings.destination, update.settings.destination);
}

#[test]
fn validation_keeps_temporary_inputs_and_revision() {
    let settings = Settings {
        revision: 4,
        destination: Some(online(Kind::Webdav)),
        ..Settings::default()
    };
    let mut draft = draft(&settings);
    draft.secret = "temporary credential".into();
    draft.passphrase = "new passphrase".into();
    draft.confirmation = "different passphrase".into();
    assert_eq!(
        draft.update(&settings, Mode::Storage),
        Err("backup_passphrase_mismatch")
    );
    assert_eq!(draft.secret, "temporary credential");
    assert_eq!(draft.passphrase, "new passphrase");
    assert_eq!(settings.revision, 4);
    draft.confirmation = draft.passphrase.clone();
    let update = draft.update(&settings, Mode::Storage).unwrap();
    assert_eq!(update.settings, settings);
    assert_eq!(
        update.secret,
        SecretUpdate::Replace(Secret::new("temporary credential".into()))
    );
    assert_eq!(
        update.passphrase,
        SecretUpdate::Replace(Secret::new("new passphrase".into()))
    );
    assert!(draft.dirty(&settings, Mode::Storage));
}
