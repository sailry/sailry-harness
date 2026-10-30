use super::*;
use crate::telemetry::{Collector, configure};
use sailry_protocol::{conversation::Provider, search, telemetry::Settings};
use serde_json::json;

fn provider() -> Provider {
    serde_json::from_value(json!({
        "id":sailry_protocol::ProviderId::new(),"revision":1,"name":"PRIVATE_NAME",
        "api":"responses","authentication":"api_key","endpoint":"https://private.invalid",
        "enabled":true,"default_model":"PRIVATE_MODEL","credential":null
    }))
    .unwrap()
}

fn start(profile: &Path) -> (Call, Request, Search) {
    (
        Call::new(
            Some(profile),
            Tool::builtin("read_session"),
            &CancellationToken::new(),
        )
        .unwrap(),
        Request::direct(Some(profile), &provider(), "PRIVATE_MODEL").unwrap(),
        Search::new(
            Some(profile),
            Some(search::Selection::Driver(search::Driver::Exa)),
        )
        .unwrap(),
    )
}

fn finish((call, mut request, search): (Call, Request, Search)) {
    call.finish(ToolResult::Success, None);
    request.status(200);
    request.tokens(Tokens {
        input: 2,
        output: 1,
        ..Default::default()
    });
    request.finish(RequestResult::Success, None);
    search.finish(&Ok(search::Page {
        driver: search::Driver::Exa,
        results: vec![],
        summary: None,
        untrusted: true,
        fetch_hint: String::new(),
    }));
}

#[test]
fn records_current_operations_once() {
    let profile = tempfile::tempdir().unwrap();
    let collector = Collector::register(&profile).unwrap();
    configure(profile.path(), &Settings::default());
    collector.payloads();
    finish(start(profile.path()));
    let payloads = collector.payloads();
    assert_eq!(payloads.len(), 3);
    for batch in &payloads {
        assert_eq!(batch["data"][0]["count"], 1);
        assert_eq!(batch["data"][0]["result"], "success");
    }
    let model = payloads
        .iter()
        .find(|batch| batch["table"] == "model_requests")
        .unwrap();
    assert_eq!(model["data"][0]["retries"], 0);
    assert_eq!(model["data"][0]["http_status_class"], "2xx");
    assert_eq!(model["data"][0]["input"], 2);
}

#[test]
fn discards_operations_after_consent_changes() {
    for reset in [false, true] {
        for cancelled in [false, true] {
            let profile = tempfile::tempdir().unwrap();
            let collector = Collector::register(&profile).unwrap();
            let mut settings = Settings::default();
            configure(profile.path(), &settings);
            collector.payloads();
            let operations = start(profile.path());
            if reset {
                settings.installation_id = uuid::Uuid::new_v4();
            } else {
                settings.metrics = false;
                configure(profile.path(), &settings);
                settings.metrics = true;
            }
            configure(profile.path(), &settings);
            if cancelled {
                drop(operations);
            } else {
                finish(operations);
            }
            assert!(collector.payloads().is_empty());
        }
    }
}

#[test]
fn rejects_replaced_profile_collectors() {
    let profile = tempfile::tempdir().unwrap();
    let collector = Collector::register(&profile).unwrap();
    let settings = Settings::default();
    configure(profile.path(), &settings);
    let operations = start(profile.path());
    drop(collector);
    let collector = Collector::register(&profile).unwrap();
    configure(profile.path(), &settings);
    collector.payloads();
    finish(operations);
    assert!(collector.payloads().is_empty());
}

#[test]
fn dropped_operations_record_cancellation() {
    let profile = tempfile::tempdir().unwrap();
    let collector = Collector::register(&profile).unwrap();
    configure(profile.path(), &Settings::default());
    collector.payloads();
    drop(start(profile.path()));
    let payloads = collector.payloads();
    assert_eq!(payloads.len(), 3);
    for batch in payloads {
        assert_eq!(batch["data"][0]["result"], "cancelled");
        assert_eq!(batch["data"][0]["count"], 1);
    }
}
