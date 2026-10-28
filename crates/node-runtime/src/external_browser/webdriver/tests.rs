use super::*;
use sailry_protocol::ErrorCode;
use serde_json::{Value, json};

fn config() -> Config {
    Config {
        webdriver_url: "http://127.0.0.1:9".into(),
        viewport_width: 1280,
        viewport_height: 900,
        page_load_timeout_secs: 25,
        script_timeout_secs: 15,
        implicit_wait_secs: 0,
        browser_args: vec![
            "--user-data-dir=/tmp/profile".into(),
            "--no-first-run".into(),
        ],
        chrome_options: std::collections::BTreeMap::from([
            ("binary".into(), json!("/tmp/chromium")),
            (
                "prefs".into(),
                json!({"download.prompt_for_download":false}),
            ),
        ]),
    }
}

#[test]
fn preserves_chromium_options() {
    let capabilities = Session::new(config()).capabilities().unwrap();
    let capabilities = serde_json::to_value(capabilities).unwrap();
    let options = &capabilities["goog:chromeOptions"];
    assert_eq!(options["binary"], "/tmp/chromium");
    assert_eq!(options["prefs"]["download.prompt_for_download"], false);
    let arguments = options["args"].as_array().unwrap();
    for argument in [
        "--headless=new",
        "--disable-dev-shm-usage",
        "--user-data-dir=/tmp/profile",
        "--no-first-run",
    ] {
        assert!(arguments.contains(&Value::from(argument)));
    }
    assert!(!arguments.contains(&Value::from("--no-sandbox")));
}

#[test]
fn rejects_replaced_arguments() {
    let mut config = config();
    config
        .chrome_options
        .insert("args".into(), json!(["--no-sandbox"]));
    let error = Session::new(config).capabilities().unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(error.message.contains("cannot set `args`"));
}

#[tokio::test]
async fn requires_owner_start() {
    let session = Session::new(config());
    let error = session.current_url().await.unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(error.message.contains("not running"));
    assert!(session.driver.read().await.is_none());
    session.stop().await.unwrap();
}
