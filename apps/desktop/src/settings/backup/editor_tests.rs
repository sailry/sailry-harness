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
