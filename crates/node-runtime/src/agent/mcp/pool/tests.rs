use super::*;

fn package() -> sailry_protocol::plugin::Summary {
    serde_json::from_value(json!({
        "name":"example","digest":"initial","revision":1,"settings_revision":1,
        "enabled":true,"version":"0.1.0","description":null
    }))
    .unwrap()
}

#[test]
fn ignores_unchanged_configuration() {
    let pool = Pool::default();
    let summary = package();
    assert!(pool.observe(&summary));
    assert!(!pool.observe(&summary));
    assert!(pool.reusable(&summary.reference()));
}

#[test]
fn preserves_newer_configuration() {
    let pool = Pool::default();
    let original = package();
    assert!(pool.observe(&original));
    let mut current = original.clone();
    current.settings_revision += 1;
    assert!(pool.observe(&current));
    assert!(!pool.observe(&original));
    assert!(!pool.reusable(&original.reference()));
    assert!(pool.reusable(&current.reference()));
}

#[test]
fn tracks_enable_changes() {
    let pool = Pool::default();
    let mut summary = package();
    assert!(pool.observe(&summary));
    summary.revision += 1;
    summary.enabled = false;
    assert!(pool.observe(&summary));
    assert!(!pool.reusable(&summary.reference()));
    summary.revision += 1;
    summary.enabled = true;
    assert!(pool.observe(&summary));
    assert!(pool.reusable(&summary.reference()));
}

#[test]
fn excludes_absent_packages_after_inventory() {
    let pool = Pool::default();
    pool.finish_inventory();
    assert!(!pool.reusable(&package().reference()));
}
