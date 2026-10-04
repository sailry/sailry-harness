use super::*;

#[test]
fn coalesces_changes_and_ends_with_the_view() {
    let fixture = Fixture::new(false);
    fixture.package();
    let package = fixture.install(0);
    let host = Host::new(
        fixture.binding.client.clone(),
        Context {
            invocation: None,
            turn: None,
            surface: Default::default(),
            package: package.summary.reference(),
            worktree: None,
            session: None,
        },
        fixture.runtime.handle().clone(),
        false,
        None,
    );
    let module = host.sdk();
    let first = complete(&fixture, &module, "nextChange", json!([]));
    assert_eq!(first["connected"], false);
    let pending = module
        .begin("nextChange", &args(json!([first["cursor"]])))
        .unwrap();
    assert!(module.begin("nextChange", &args(json!([]))).is_err());
    host.observe(1, true);
    host.observe(7, true);
    let next = decode(&fixture.runtime.block_on(pending).unwrap()).unwrap();
    assert_eq!(
        next,
        json!({"cursor":"7:true:0","connected":true,"entry":"0"})
    );
    let pending = module
        .begin("nextChange", &args(json!([next["cursor"]])))
        .unwrap();
    host.enter();
    host.enter();
    host.observe(7, true);
    let next = decode(&fixture.runtime.block_on(pending).unwrap()).unwrap();
    assert_eq!(
        next,
        json!({"cursor":"7:true:2","connected":true,"entry":"2"})
    );
    host.observe(8, true);
    let next = complete(&fixture, &module, "nextChange", json!([next["cursor"]]));
    assert_eq!(
        next,
        json!({"cursor":"8:true:2","connected":true,"entry":"2"})
    );
    let pending = module
        .begin("nextChange", &args(json!([next["cursor"]])))
        .unwrap();
    host.close();
    assert!(fixture.runtime.block_on(pending).is_err());
    assert!(module.begin("nextChange", &args(json!([]))).is_err());
    fixture.close();
}
