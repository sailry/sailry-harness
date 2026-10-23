use super::*;
mod service;

#[test]
fn restores_only_valid_existing_credentials() {
    for case in [
        "valid",
        "expired",
        "missing",
        "pending",
        "not_connected",
        "changed_target",
        "ordinary",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let host = crate::plugins::Host::new(Some(directory.path().canonicalize().unwrap()));
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../../../schema.sql"))
            .unwrap();
        let mut info = if case == "ordinary" {
            host.install_bundled("notion").unwrap()
        } else {
            host.install_mcp(
                "mcp-fixture",
                &plugin::mcp::Definition::StreamableHttp {
                    url: "https://example.com/mcp".into(),
                    headers: vec![],
                },
            )
            .unwrap()
        };
        crate::store::plugins::save(&db, &info, true).unwrap();
        let declaration = registry::declarations(&info)
            .into_iter()
            .find(|declaration| {
                declaration
                    .auth
                    .iter()
                    .any(|auth| matches!(auth, Auth::McpOauth { .. }))
            })
            .unwrap();
        let command = Command::StartConnectorLogin {
            package: info.summary.reference(),
            connector: declaration.id,
            connection: None,
            expected_revision: None,
            method: Kind::McpOauth,
            input: Input::default(),
        };
        let (mut record, _) = super::super::create(&db, host.profile(), &command).unwrap();
        record.enabled = false;
        record.state = if case == "not_connected" {
            State::NotConnected
        } else {
            State::Connected
        };
        record.grant = (case != "missing").then(|| Grant {
            access: Secret::new("private-retained-grant".into()),
            refresh: None,
            expires_at_ms: Some(if case == "expired" { 1 } else { u64::MAX / 2 }),
            extra: BTreeMap::new(),
        });
        if case == "changed_target" {
            record
                .bound
                .as_mut()
                .unwrap()
                .origins
                .push("https://changed.example.com".into());
        }
        if case == "pending" {
            record.state = State::Connecting;
        }
        super::super::save(&db, &record).unwrap();
        let projected = super::super::project(&db, host.profile(), &record).unwrap();
        let expected = match case {
            "expired" => State::Expired,
            "missing" | "not_connected" => State::NotConnected,
            "pending" => State::Connecting,
            "changed_target" => State::NeedsConfirmation,
            _ => State::Connected,
        };
        assert_eq!(projected.state, expected, "{case}");
        if case != "ordinary" {
            use plugin::mcp::Availability;
            let expected = match expected {
                State::Expired => Availability::AuthorizationExpired,
                State::NotConnected | State::Connecting => Availability::ConnectionRequired,
                State::NeedsConfirmation => Availability::ConsentRequired,
                _ => Availability::ConnectionDisabled,
            };
            assert_eq!(
                crate::store::plugins::mcp::project(&db, host.profile(), info.clone())
                    .unwrap()
                    .mcp[0]
                    .availability,
                expected,
                "{case}"
            );
        }
        // Enabling a custom server is independent of enabling its package.
        info.summary.enabled = false;
        crate::store::plugins::save(&db, &info, true).unwrap();
        let before = encode(&record).unwrap();
        let server = info.mcp[0].name.clone();
        let changed = enable(&db, host.profile(), &info, &server).unwrap();
        let saved = read(&db, record.id).unwrap();
        assert_eq!(changed, case == "valid", "{case}");
        if changed {
            assert!(saved.enabled);
            assert_eq!(
                saved.grant.as_ref().unwrap().access.expose(),
                "private-retained-grant"
            );
            assert_eq!(saved.revision, record.revision + 1);
            assert_eq!(saved.auth_revision, saved.revision);
            assert!(!required(&db, &info.summary.name).unwrap().summary.enabled);
            assert!(!enable(&db, host.profile(), &info, &server).unwrap());
        } else {
            assert_eq!(encode(&saved).unwrap(), before, "{case}");
        }
    }
}
