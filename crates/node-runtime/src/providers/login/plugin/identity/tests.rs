use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{EncodingKey, Header};
use serde_json::{Value, json};

const ISSUER: &str = "https://auth.openai.com";
const CLIENT: &str = "oaiapp_fixture";

// RFC 8032 section 7.1 test vector 1. This public test seed is not a credential.
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];
const PUBLIC: [u8; 32] = [
    0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07, 0x3a,
    0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07, 0x51, 0x1a,
];

fn signed(claims: &Value) -> Secret {
    let mut key = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    key.extend_from_slice(&SEED);
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some("fixture-key".into());
    Secret::new(jsonwebtoken::encode(&header, claims, &EncodingKey::from_ed_der(&key)).unwrap())
}

fn keys() -> JwkSet {
    serde_json::from_value(json!({"keys":[{"kty":"OKP", "crv":"Ed25519", "alg":"EdDSA",
        "kid":"fixture-key", "use":"sig", "key_ops":["verify"], "x":URL_SAFE_NO_PAD.encode(PUBLIC)}]})).unwrap()
}

fn claims() -> Value {
    let now = now_ms().unwrap() / 1000;
    json!({"iss":ISSUER,"aud":CLIENT,"sub":"verified-subject","email":"user@example.com",
        "nonce":"nonce","iat":now,"exp":now+3600})
}

#[test]
fn validates_signed_identity_and_nonce() {
    let identity = verified(&signed(&claims()), CLIENT, Some("nonce"), ISSUER, &keys()).unwrap();
    assert_eq!(identity["sub"], "verified-subject");
    assert_eq!(identity["email"].as_str(), Some("user@example.com"));
    assert!(identity.get("chatgpt_account_id").is_none());
}

#[test]
fn retains_verified_custom_claims() {
    let client = "app_EMoamEEZ73f0CkXaXp7hrann";
    for nested in [false, true] {
        let mut claims = claims();
        claims["aud"] = json!(client);
        claims.as_object_mut().unwrap().remove("nonce");
        if nested {
            claims["https://api.openai.com/auth"] = json!({"chatgpt_account_id":"workspace"});
        } else {
            claims["chatgpt_account_id"] = json!("workspace");
        }
        let token = signed(&claims);
        let identity = verified(&token, client, None, ISSUER, &keys()).unwrap();
        assert_eq!(identity["sub"], "verified-subject");
        assert_eq!(
            identity
                .pointer(if nested {
                    "/https:~1~1api.openai.com~1auth/chatgpt_account_id"
                } else {
                    "/chatgpt_account_id"
                })
                .and_then(Value::as_str),
            Some("workspace")
        );
        assert!(verified(&token, CLIENT, None, ISSUER, &keys()).is_err());
        assert!(verified(&token, client, Some("nonce"), ISSUER, &keys()).is_err());
    }
}

#[test]
fn rejects_invalid_signature_and_key() {
    let token = signed(&claims());
    let mut segments: Vec<_> = token.expose().split('.').map(str::to_owned).collect();
    let mut signature = URL_SAFE_NO_PAD.decode(&segments[2]).unwrap();
    signature[0] ^= 1;
    segments[2] = URL_SAFE_NO_PAD.encode(signature);
    assert!(
        verified(
            &Secret::new(segments.join(".")),
            CLIENT,
            Some("nonce"),
            ISSUER,
            &keys()
        )
        .is_err()
    );
    let empty = JwkSet { keys: vec![] };
    assert!(verified(&token, CLIENT, Some("nonce"), ISSUER, &empty).is_err());
    let mut keys = keys();
    keys.keys[0].common.public_key_use = Some(jsonwebtoken::jwk::PublicKeyUse::Encryption);
    assert!(verified(&token, CLIENT, Some("nonce"), ISSUER, &keys).is_err());
}

#[test]
fn rejects_wrong_or_missing_claims() {
    for (key, value) in [
        ("iss", json!("https://other.example")),
        ("aud", json!("oaiapp_other")),
        ("nonce", json!("other")),
        ("sub", json!("")),
        ("exp", json!(1)),
        ("iat", json!(now_ms().unwrap() / 1000 + 60)),
        ("nbf", json!(now_ms().unwrap() / 1000 + 60)),
        ("azp", json!("oaiapp_other")),
    ] {
        let mut claims = claims();
        claims[key] = value;
        assert!(
            verified(&signed(&claims), CLIENT, Some("nonce"), ISSUER, &keys()).is_err(),
            "{key}"
        );
    }
    for key in ["iss", "aud", "sub", "exp", "iat", "nonce"] {
        let mut claims = claims();
        claims.as_object_mut().unwrap().remove(key);
        assert!(
            verified(&signed(&claims), CLIENT, Some("nonce"), ISSUER, &keys()).is_err(),
            "{key}"
        );
    }
    let mut multiple = claims();
    multiple["aud"] = json!([CLIENT, "another-client"]);
    assert!(verified(&signed(&multiple), CLIENT, Some("nonce"), ISSUER, &keys()).is_err());
    multiple["azp"] = json!(CLIENT);
    assert!(verified(&signed(&multiple), CLIENT, Some("nonce"), ISSUER, &keys()).is_ok());
}

#[test]
fn discovery_refuses_cross_origin_signing_endpoints() {
    for (issuer, jwks_uri) in [
        (
            "https://other.example",
            "https://auth.openai.com/.well-known/jwks.json",
        ),
        (ISSUER, "http://auth.openai.com/.well-known/jwks.json"),
        (ISSUER, "https://other.example/.well-known/jwks.json"),
        (ISSUER, "https://user@auth.openai.com/.well-known/jwks.json"),
        (
            ISSUER,
            "https://auth.openai.com/.well-known/jwks.json#fragment",
        ),
    ] {
        assert!(
            Discovery {
                issuer: issuer.into(),
                jwks_uri: jwks_uri.into()
            }
            .validate(ISSUER)
            .is_err()
        );
    }
    assert!(
        Discovery {
            issuer: ISSUER.into(),
            jwks_uri: format!("{ISSUER}/.well-known/jwks.json")
        }
        .validate(ISSUER)
        .is_ok()
    );
}
