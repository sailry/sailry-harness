use super::*;

#[test]
fn defaults_missing_editor_switch() {
    let mut value = serde_json::to_value(Settings::default()).unwrap();
    value.as_object_mut().unwrap().remove("editor_enabled");
    value["after_edit"] = false.into();
    let settings: Settings = serde_json::from_value(value).unwrap();
    assert!(!settings.editor_enabled);
    assert!(settings.enabled);
    assert!(!settings.after_edit);
    let mut value = serde_json::to_value(&settings).unwrap();
    value["editor_enabled"] = true.into();
    assert!(
        serde_json::from_value::<Settings>(value.clone())
            .unwrap()
            .editor_enabled
    );
    value["editor_enabled"] = "true".into();
    assert!(serde_json::from_value::<Settings>(value).is_err());
}

#[test]
fn defaults_missing_master_switch() {
    let mut value = serde_json::to_value(Settings::default()).unwrap();
    value.as_object_mut().unwrap().remove("enabled");
    value["after_edit"] = false.into();
    let settings: Settings = serde_json::from_value(value).unwrap();
    assert!(settings.enabled);
    assert!(!settings.after_edit);
    let mut value = serde_json::to_value(&settings).unwrap();
    value["enabled"] = false.into();
    assert!(!serde_json::from_value::<Settings>(value).unwrap().enabled);
}
