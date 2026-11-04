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
fn storage_and_schedule_drafts() {
    for (scenario, check) in [
        (
            "selects_one_destination_without_erasing_configuration",
            (|_scenario: &str| {
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
                    assert_eq!(
                        settings.online_enabled,
                        tab < 2,
                        "selects one destination without erasing configuration"
                    );
                    assert_eq!(
                        settings.local.as_ref().unwrap().enabled,
                        tab == 2,
                        "selects one destination without erasing configuration"
                    );
                    assert_eq!(
                        storage_tab(&settings),
                        tab,
                        "selects one destination without erasing configuration"
                    );
                    assert_eq!(
                        settings.schedule, original.schedule,
                        "selects one destination without erasing configuration"
                    );
                    assert_eq!(
                        settings.local.as_ref().unwrap().directory,
                        "/node/backups",
                        "selects one destination without erasing configuration"
                    );
                    assert_eq!(
                        settings.destination.as_ref().unwrap().kind,
                        if tab == 0 { Kind::Webdav } else { Kind::S3 },
                        "selects one destination without erasing configuration"
                    );
                }
                assert!(
                    original.online_enabled,
                    "selects one destination without erasing configuration"
                );
                assert!(
                    original.local.as_ref().unwrap().enabled,
                    "selects one destination without erasing configuration"
                );
            }) as fn(&str),
        ),
        (
            "rejects_incomplete_selected_storage",
            (|_scenario: &str| {
                let original = Settings {
                    destination: Some(online(Kind::S3)),
                    ..Settings::default()
                };
                let mut settings = original.clone();
                assert_eq!(
                    storage(&mut settings, 2, online(Kind::Webdav), ""),
                    Err("backup_invalid_target"),
                    "rejects incomplete selected storage"
                );
                assert_eq!(settings, original, "rejects incomplete selected storage");
                let mut missing_bucket = online(Kind::S3);
                missing_bucket.bucket.clear();
                assert_eq!(
                    storage(&mut settings, 1, missing_bucket, "/node/backups"),
                    Err("backup_invalid"),
                    "rejects incomplete selected storage"
                );
                assert_eq!(settings, original, "rejects incomplete selected storage");
                let mut missing_endpoint = online(Kind::Webdav);
                missing_endpoint.endpoint.clear();
                assert_eq!(
                    storage(&mut settings, 0, missing_endpoint, "/node/backups"),
                    Err("backup_invalid"),
                    "rejects incomplete selected storage"
                );
                assert_eq!(settings, original, "rejects incomplete selected storage");
            }),
        ),
        (
            "preserves_untouched_defaults",
            (|_scenario: &str| {
                let settings = Settings::default();
                let draft = draft(&settings);
                for mode in [Mode::Schedule, Mode::Storage] {
                    assert!(
                        !draft.dirty(&settings, mode),
                        "preserves untouched defaults"
                    );
                    let update = draft.update(&settings, mode).unwrap();
                    assert_eq!(update.settings, settings, "preserves untouched defaults");
                    assert_eq!(
                        update.identity,
                        SecretUpdate::Keep,
                        "preserves untouched defaults"
                    );
                    assert_eq!(
                        update.secret,
                        SecretUpdate::Keep,
                        "preserves untouched defaults"
                    );
                    assert_eq!(
                        update.passphrase,
                        SecretUpdate::Keep,
                        "preserves untouched defaults"
                    );
                }
            }),
        ),
        (
            "schedule_saves_never_commit_storage_drafts",
            (|_scenario: &str| {
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
                assert_eq!(
                    update.settings.revision, 8,
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    update.settings.destination, settings.destination,
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    update.settings.local, settings.local,
                    "schedule saves never commit storage drafts"
                );
                assert!(
                    update.settings.online_enabled,
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    update.settings.schedule,
                    Some(draft.schedule.clone()),
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    update.identity,
                    SecretUpdate::Keep,
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    update.secret,
                    SecretUpdate::Keep,
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    update.session_token,
                    SecretUpdate::Keep,
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    update.passphrase,
                    SecretUpdate::Keep,
                    "schedule saves never commit storage drafts"
                );
                assert_eq!(
                    draft.directory, "/unsaved/storage",
                    "schedule saves never commit storage drafts"
                );
            }),
        ),
        (
            "storage_saves_preserve_current_schedule",
            (|_scenario: &str| {
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
                assert!(
                    draft.dirty(&settings, Mode::Storage),
                    "storage saves preserve current schedule"
                );
                let update = draft.update(&settings, Mode::Storage).unwrap();
                assert_eq!(
                    update.settings.revision, 8,
                    "storage saves preserve current schedule"
                );
                assert_eq!(
                    update.settings.schedule, None,
                    "storage saves preserve current schedule"
                );
                assert_eq!(
                    update.settings.destination.as_ref().unwrap().endpoint,
                    "https://changed.example.test",
                    "storage saves preserve current schedule"
                );
                assert_eq!(
                    update.identity,
                    SecretUpdate::Replace(Secret::new("access key".into())),
                    "storage saves preserve current schedule"
                );
                settings.revision = 9;
                settings.schedule = Some(Schedule {
                    enabled: true,
                    timezone: "Asia/Shanghai".into(),
                    hour: 7,
                    minute: 30,
                });
                let rebased = draft.update(&settings, Mode::Storage).unwrap();
                assert_eq!(
                    rebased.settings.revision, 9,
                    "storage saves preserve current schedule"
                );
                assert_eq!(
                    rebased.settings.schedule, settings.schedule,
                    "storage saves preserve current schedule"
                );
                assert_eq!(
                    rebased.identity, update.identity,
                    "storage saves preserve current schedule"
                );
                assert_eq!(
                    rebased.settings.destination, update.settings.destination,
                    "storage saves preserve current schedule"
                );
            }),
        ),
        (
            "validation_keeps_temporary_inputs_and_revision",
            (|_scenario: &str| {
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
                    Err("backup_passphrase_mismatch"),
                    "validation keeps temporary inputs and revision"
                );
                assert_eq!(
                    draft.secret, "temporary credential",
                    "validation keeps temporary inputs and revision"
                );
                assert_eq!(
                    draft.passphrase, "new passphrase",
                    "validation keeps temporary inputs and revision"
                );
                assert_eq!(
                    settings.revision, 4,
                    "validation keeps temporary inputs and revision"
                );
                draft.confirmation = draft.passphrase.clone();
                let update = draft.update(&settings, Mode::Storage).unwrap();
                assert_eq!(
                    update.settings, settings,
                    "validation keeps temporary inputs and revision"
                );
                assert_eq!(
                    update.secret,
                    SecretUpdate::Replace(Secret::new("temporary credential".into())),
                    "validation keeps temporary inputs and revision"
                );
                assert_eq!(
                    update.passphrase,
                    SecretUpdate::Replace(Secret::new("new passphrase".into())),
                    "validation keeps temporary inputs and revision"
                );
                assert!(
                    draft.dirty(&settings, Mode::Storage),
                    "validation keeps temporary inputs and revision"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
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
