use sailry_protocol::plugin::{Extension, Scope, ui};

pub(super) fn valid(extension: &Extension) -> bool {
    let status = extension
        .ui
        .iter()
        .any(|entry| entry.slot == ui::Slot::Status);
    let host = extension
        .host
        .as_ref()
        .and_then(|host| host.status.as_ref());
    status == host.is_some()
        && (!status || extension.scope == Scope::Host)
        && (extension
            .ui
            .iter()
            .all(|entry| entry.slot == ui::Slot::Status && entry.kind == ui::Kind::Button)
            || extension
                .desktop
                .as_ref()
                .is_some_and(|desktop| desktop.ui_entry.is_some()))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    fn accepted(extension: Value) -> bool {
        let manifest = super::super::parse(
            &serde_json::to_vec(&json!({
                "$schema":super::super::SCHEMA,"name":"status-fixture",
                "extensions":{"dev.sailry.platform":extension}
            }))
            .unwrap(),
        )
        .unwrap();
        manifest.issues.is_empty() && manifest.extension.is_some()
    }

    #[test]
    fn requires_a_host_owner() {
        let base = json!({
            "api_version":"v1","scope":"host",
            "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
                "handlers":["read"],"read_only":["read"],"status":{"read":"read"}},
            "ui":[{"id":"badge","slot":"status","kind":"button","label":{"label":"Badge"},"handler":"badge"}]
        });
        assert!(accepted(base.clone()));
        let mut missing = base.clone();
        missing["host"].as_object_mut().unwrap().remove("status");
        assert!(!accepted(missing));
        let mut desktop = base.clone();
        desktop["scope"] = json!("desktop");
        assert!(!accepted(desktop));
        let mut empty = base;
        empty["ui"] = json!([]);
        assert!(!accepted(empty));
    }

    #[test]
    fn requires_views_for_popovers() {
        let mut extension = json!({
            "api_version":"v1","scope":"host",
            "host":{"entry":"dev.sailry.platform/main.js","resources":["dev.sailry.platform/main.js"],
                "handlers":["read"],"read_only":["read"],"status":{"read":"read"}},
            "ui":[{"id":"badge","slot":"status","kind":"popover","label":{"label":"Badge"},"handler":"badge"}]
        });
        assert!(!accepted(extension.clone()));
        extension["desktop"] = json!({
            "ui_entry":"dev.sailry.platform/view.js","resources":["dev.sailry.platform/view.js"]
        });
        assert!(accepted(extension.clone()));
        extension["ui"][0]["slot"] = json!("composer");
        extension["host"].as_object_mut().unwrap().remove("status");
        assert!(accepted(extension.clone()));
        extension.as_object_mut().unwrap().remove("desktop");
        assert!(!accepted(extension));
    }
}
