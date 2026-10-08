use super::*;

#[test]
fn persists_without_changing_selection() {
    assert!(Data::default().theme_effects.unwrap_or(true));
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("preferences.json");
    for enabled in [false, true] {
        let mut preferences = Preferences::open(path.clone());
        let selection = preferences.data.appearance.clone();
        preferences.data.theme_effects = Some(enabled);
        preferences.save();
        assert_eq!(preferences.error, None);
        let restored = Preferences::open(path.clone());
        assert_eq!(restored.data.theme_effects, Some(enabled));
        assert_eq!(restored.data.appearance, selection);
        assert_eq!(restored.data.version, 1);
    }
}
