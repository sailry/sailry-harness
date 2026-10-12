use super::*;

fn pending(redirect: &str) -> Pending {
    Pending {
        redirect: redirect.into(),
        verifier: Secret::new("synthetic-verifier".into()),
    }
}

#[test]
fn validates_exact_redirects() {
    for redirect in [
        COPY_REDIRECT,
        "http://127.0.0.1:40000/callback",
        "http://localhost:40000/callback",
    ] {
        assert!(Pending::new(redirect.into()).is_ok(), "{redirect}");
    }
    for redirect in [
        "https://example.test/callback",
        "http://127.0.0.1/callback",
        "http://localhost:40000/auth/callback",
        "http://localhost:40000/callback?code=one",
        "http://user@localhost:40000/callback",
    ] {
        assert!(Pending::new(redirect.into()).is_err(), "{redirect}");
    }
}

#[test]
fn sends_pi_challenge_and_scopes() {
    let pending = pending(COPY_REDIRECT);
    let url = Url::parse(&pending.url("https://claude.ai").unwrap()).unwrap();
    let fields = url.query_pairs().collect::<BTreeMap<_, _>>();
    assert_eq!(url.path(), "/oauth/authorize");
    assert_eq!(fields["client_id"], CLIENT_ID);
    assert_eq!(fields["scope"], SCOPES);
    assert_eq!(fields["state"], pending.verifier.expose());
    assert_eq!(fields["code_challenge_method"], "S256");
    assert_eq!(
        fields["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(pending.verifier.expose().as_bytes()))
    );
}

#[test]
fn accepts_browser_and_copy_codes() {
    for input in [
        "synthetic-code",
        "synthetic-code#synthetic-verifier",
        "code=synthetic-code&state=synthetic-verifier",
        "https://platform.claude.com/oauth/code/callback?code=synthetic-code&state=synthetic-verifier",
    ] {
        assert_eq!(
            pending(COPY_REDIRECT)
                .code(&Secret::new(input.into()))
                .unwrap()
                .expose(),
            "synthetic-code"
        );
    }
    let callback = "http://127.0.0.1:40000/callback?code=synthetic-code&state=synthetic-verifier";
    assert_eq!(
        pending("http://127.0.0.1:40000/callback")
            .code(&Secret::new(callback.into()))
            .unwrap()
            .expose(),
        "synthetic-code"
    );
}

#[test]
fn rejects_invalid_callbacks() {
    for input in [
        "",
        "code=first&code=second",
        "code=value&state=wrong",
        "code=value&error=access_denied",
        "synthetic-code#wrong",
        "code with spaces",
        "https://example.test/callback?code=value",
        "https://platform.claude.com/oauth/code/callback?code=value#fragment",
    ] {
        assert!(
            pending(COPY_REDIRECT)
                .code(&Secret::new(input.into()))
                .is_err(),
            "{input}"
        );
    }
}

#[test]
fn retains_rotating_grants() {
    let tokens = Tokens {
        access_token: Secret::new("synthetic-access".into()),
        refresh_token: Secret::new("synthetic-refresh".into()),
        expires_in: 3600,
    };
    let before = now_ms().unwrap();
    let mut grant = tokens.grant().unwrap();
    assert_eq!(grant.authentication(), Authentication::ClaudeOAuth);
    assert!(grant.expires_at_ms() >= before + 3300 * 1000);
    assert!(!grant.renewal_pending());
    grant.begin_renewal();
    assert!(grant.renewal_pending());
    let encoded = grant.encode().unwrap();
    let restored = Grant::decode(Authentication::ClaudeOAuth, &encoded).unwrap();
    assert!(restored.renewal_pending());
    assert!(transferable(Authentication::ClaudeOAuth).is_err());
}
