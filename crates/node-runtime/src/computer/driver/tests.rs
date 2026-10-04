use super::*;

mod expiry {
    use super::*;

    #[test]
    fn retains_recent_activity() {
        let now = Instant::now();
        let lease = Lease {
            created: now - Duration::from_secs(TTL_SECONDS - 60),
            used: now - Duration::from_secs(IDLE_TTL_SECONDS - 60),
        };
        assert!(!lease.expired());
    }

    #[test]
    fn ends_idle_authority() {
        let now = Instant::now();
        let lease = Lease {
            created: now - Duration::from_secs(IDLE_TTL_SECONDS),
            used: now - Duration::from_secs(IDLE_TTL_SECONDS),
        };
        assert!(lease.expired());
    }

    #[test]
    fn does_not_extend_absolute_authority() {
        let now = Instant::now();
        let lease = Lease {
            created: now - Duration::from_secs(TTL_SECONDS),
            used: now,
        };
        assert!(lease.expired());
    }
}

mod refusals {
    use super::*;
    use serde_json::json;

    #[test]
    fn releases_terminal_authority() {
        assert!(closed(Some("authorization_revoked"), &json!({})));
        for message in [
            "Permission denied: authorization context expired",
            "authorization context expired",
            "authorization context idle timeout exceeded",
        ] {
            assert!(closed(
                Some("permission_denied"),
                &json!({
                    "structuredContent":{"refusal":{"message":message}}
                })
            ));
        }
    }

    #[test]
    fn retains_ordinary_denials() {
        for message in [
            "public session substitution does not match the bound authorization context",
            "Permission denied by user policy",
            "No current snapshot contains a screenshot owned by this session",
        ] {
            let output = json!({"structuredContent":{"refusal":{"message":message}}});
            assert!(!closed(Some("permission_denied"), &output));
            assert!(!closed(Some("screenshot_not_owned"), &output));
        }
    }
}
