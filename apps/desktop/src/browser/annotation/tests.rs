use super::*;
use core::prelude::v1::test;

fn element() -> Element {
    Element {
        url: "https://example.com/page".into(),
        selector: "#heading".into(),
        text: "Visible heading".into(),
        bounds: [10., 20., 180., 30.],
        viewport: [640., 480.],
        truncated: false,
    }
}

#[test]
fn accepts_only_original_context_and_bounded_note() {
    let item = element();
    let value = serde_json::json!({"page_element":item,"annotation":"Replace this heading"});
    assert!(Reference::matches(&value.to_string(), &item));
    let mut changed = value.clone();
    changed["page_element"]["url"] = "https://other.example/".into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed = value.clone();
    changed["page_element"]["text"] = "Forged heading".into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed = value.clone();
    changed["annotation"] = "x".repeat(4097).into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed["annotation"] = "\0".into();
    assert!(!Reference::matches(&changed.to_string(), &item));
    changed["annotation"] = "".into();
    assert!(Reference::matches(&changed.to_string(), &item));
    changed["extra"] = true.into();
    assert!(!Reference::matches(&changed.to_string(), &item));
}

#[test]
fn rejects_unbounded_or_unusable_context() {
    let mut item = element();
    item.text = "x".repeat(16 * 1024 + 1);
    assert!(!item.validate());
    item = element();
    item.bounds[0] = f64::NAN;
    assert!(!item.validate());
    item = element();
    item.bounds[1] = 900.;
    assert!(!item.validate());
    item = element();
    item.url = "https://user:secret@example.com/".into();
    assert!(!item.validate());
    item = element();
    item.selector.clear();
    assert!(!item.validate());
}

#[test]
fn cancels_pending_selection_when_released() {
    let stop = CancellationToken::new();
    let mut state = State::default();
    state.target = Some(Target {
        token: "selected".into(),
        tab: 0,
        url: element().url,
        lease: 0,
        stop: stop.clone(),
    });
    drop(state);
    assert!(stop.is_cancelled());
}
