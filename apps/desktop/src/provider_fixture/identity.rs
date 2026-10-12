//! Published signing test vectors supply isolated, verified account metadata.
use crate::discovery_fixture::Request;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer as _, SigningKey};
use serde_json::{Value, json};
use sha2::Digest as _;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

const CLIENT: &str = "oaiapp_fixture";
pub(super) const CODEX_CLIENT: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CLAUDE_CLIENT: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
// RFC 8032 section 7.1 test vector 1; these published bytes are not credentials.
const SEED: [u8; 32] = [
    0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c, 0xc4,
    0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae, 0x7f, 0x60,
];

pub(super) fn issuer(request: &Request) -> String {
    format!("http://{}", request.headers["host"])
}

pub(super) fn keys() -> Value {
    let public = SigningKey::from_bytes(&SEED).verifying_key().to_bytes();
    json!({"keys":[{"kty":"OKP","crv":"Ed25519","alg":"EdDSA","kid":"fixture-key","use":"sig","key_ops":["verify"],"x":URL_SAFE_NO_PAD.encode(public)}]})
}

fn signed(claims: Value) -> String {
    let header =
        URL_SAFE_NO_PAD.encode(json!({"alg":"EdDSA","kid":"fixture-key","typ":"JWT"}).to_string());
    let claims = URL_SAFE_NO_PAD.encode(claims.to_string());
    let message = format!("{header}.{claims}");
    let signature = SigningKey::from_bytes(&SEED).sign(message.as_bytes());
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

pub(super) fn tokens(request: &Request) -> Value {
    let form: BTreeMap<_, _> = Url::parse(&format!("http://fixture/?{}", request.body))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(form["client_id"], CLIENT);
    assert_eq!(form["resource"], "https://api.openai.com/v1");
    assert!(!form.contains_key("client_secret"));
    let mut tokens = json!({"access_token":"desktop-chatgpt-secret","refresh_token":"desktop-refresh-secret","token_type":"Bearer","expires_in":3600,"scope":SCOPES});
    if form["grant_type"] == "authorization_code" {
        let code: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&form["code"]).unwrap()).unwrap();
        assert_eq!(
            code["challenge"],
            URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(form["code_verifier"].as_bytes()))
        );
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        tokens["id_token"] = json!(signed(
            json!({"iss":issuer(request),"aud":CLIENT,"sub":"isolated-chatgpt-subject","email":"user@example.com","nonce":code["nonce"],"iat":now,"exp":now+3600})
        ));
    }
    tokens
}

pub(super) fn codex_tokens(request: &Request) -> Value {
    let form: BTreeMap<_, _> = Url::parse(&format!("http://fixture/?{}", request.body))
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    assert_eq!(form["client_id"], CODEX_CLIENT);
    assert!(!form.contains_key("resource"));
    assert!(!form.contains_key("client_secret"));
    let mut tokens = json!({"access_token":"desktop-codex-access","refresh_token":"desktop-codex-refresh","token_type":"Bearer","expires_in":3600});
    if form["grant_type"] == "authorization_code" {
        assert_eq!(
            form["redirect_uri"],
            "https://auth.openai.com/deviceauth/callback"
        );
        assert_eq!(form["code"], "desktop-codex-code");
        assert_eq!(form["code_verifier"], "desktop-codex-verifier");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        tokens["id_token"] = json!(signed(json!({
            "iss":issuer(request),"aud":CODEX_CLIENT,"sub":"isolated-codex-subject",
            "email":"device@example.com","chatgpt_account_id":"isolated-codex-workspace",
            "iat":now,"exp":now+3600
        })));
    } else {
        assert_eq!(form["grant_type"], "refresh_token");
        assert_eq!(form["refresh_token"], "desktop-codex-refresh");
        assert_eq!(form["scope"], "openid profile email");
    }
    tokens
}

pub(super) fn claude_tokens(request: &Request) -> Value {
    let body: Value = serde_json::from_str(&request.body).unwrap();
    assert_eq!(body["client_id"], CLAUDE_CLIENT);
    assert_eq!(request.headers["content-type"], "application/json");
    assert!(body.get("client_secret").is_none());
    if body["grant_type"] == "authorization_code" {
        let code: Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(body["code"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(body["state"], body["code_verifier"]);
        assert_eq!(
            code["challenge"],
            URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(
                body["code_verifier"].as_str().unwrap().as_bytes(),
            ))
        );
        assert!(
            body["redirect_uri"]
                .as_str()
                .unwrap()
                .ends_with("/callback")
        );
    } else {
        assert_eq!(body["grant_type"], "refresh_token");
        assert_eq!(body["refresh_token"], "desktop-claude-refresh");
        assert!(body.get("scope").is_none());
    }
    json!({"access_token":"sk-ant-oat01-desktop-claude-access", "refresh_token":"desktop-claude-refresh", "expires_in":3600})
}

pub(crate) fn callback(authorization: &str) -> sailry_protocol::Secret {
    let fields: BTreeMap<_, _> = Url::parse(authorization)
        .unwrap()
        .query_pairs()
        .into_owned()
        .collect();
    let (client_id, payload) = if fields["client_id"] == CLAUDE_CLIENT {
        (CLAUDE_CLIENT, json!({"challenge":fields["code_challenge"]}))
    } else {
        (
            CLIENT,
            json!({"nonce":fields["nonce"],"challenge":fields["code_challenge"]}),
        )
    };
    let code = URL_SAFE_NO_PAD.encode(payload.to_string());
    let mut callback = Url::parse(&fields["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &fields["state"])
        .append_pair("code", &code)
        .append_pair("client_id", client_id);
    sailry_protocol::Secret::new(callback.into())
}
