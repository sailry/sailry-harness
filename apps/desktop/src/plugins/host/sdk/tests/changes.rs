use super::*;

#[test]
fn scoped_observations() {
    for (scenario, check) in [
        (
            "coalesces_changes_and_ends_with_the_view",
            (|_scenario: &str| {
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
                assert_eq!(
                    first["connected"], false,
                    "coalesces changes and ends with the view"
                );
                let pending = module
                    .begin("nextChange", &args(json!([first["cursor"]])))
                    .unwrap();
                let other = module
                    .begin("nextChange", &args(json!([first["cursor"]])))
                    .unwrap();
                let next = fixture.runtime.block_on(async {
                    let mut first = std::pin::pin!(pending);
                    let mut second = std::pin::pin!(other);
                    assert!(
                        futures::poll!(&mut first).is_pending(),
                        "coalesces changes and ends with the view"
                    );
                    assert!(
                        futures::poll!(&mut second).is_pending(),
                        "coalesces changes and ends with the view"
                    );
                    host.observe(1, 0, 0, true);
                    host.observe(7, 0, 0, true);
                    let (first, second) = futures::join!(first, second);
                    let first = decode(&first.unwrap()).unwrap();
                    assert_eq!(
                        first,
                        decode(&second.unwrap()).unwrap(),
                        "coalesces changes and ends with the view"
                    );
                    first
                });
                assert_eq!(
                    next,
                    json!({"cursor":"7-0:true:0:true","revision":"7","connected":true,"entry":"0","visible":true,"terminal_tools":"0","references":"0"}),
                    "coalesces changes and ends with the view"
                );
                let pending = module
                    .begin("nextChange", &args(json!([next["cursor"]])))
                    .unwrap();
                host.enter();
                host.enter();
                host.observe(7, 0, 0, true);
                let next = decode(&fixture.runtime.block_on(pending).unwrap()).unwrap();
                assert_eq!(
                    next,
                    json!({"cursor":"7-0:true:2:true","revision":"7","connected":true,"entry":"2","visible":true,"terminal_tools":"0","references":"0"}),
                    "coalesces changes and ends with the view"
                );
                host.observe(8, 0, 0, true);
                let next = complete(&fixture, &module, "nextChange", json!([next["cursor"]]));
                assert_eq!(
                    next,
                    json!({"cursor":"8-0:true:2:true","revision":"8","connected":true,"entry":"2","visible":true,"terminal_tools":"0","references":"0"}),
                    "coalesces changes and ends with the view"
                );
                let global = module
                    .begin("nextChange", &args(json!([next["cursor"]])))
                    .unwrap();
                let first = complete(&fixture, &module, "nextChange", json!(["", "package"]));
                let pending = module
                    .begin("nextChange", &args(json!([first["cursor"], "package"])))
                    .unwrap();
                let mut pending = std::pin::pin!(pending);
                fixture.runtime.block_on(async {
                    assert!(
                        futures::poll!(&mut pending).is_pending(),
                        "coalesces changes and ends with the view"
                    );
                });
                // A shared cursor can advance without making this package dirty.
                host.observe(9, 0, 0, true);
                let next = decode(&fixture.runtime.block_on(global).unwrap()).unwrap();
                assert_eq!(
                    next["cursor"], "9-0:true:2:true",
                    "coalesces changes and ends with the view"
                );
                fixture.runtime.block_on(async {
                    assert!(
                        futures::poll!(&mut pending).is_pending(),
                        "coalesces changes and ends with the view"
                    );
                });
                host.observe(10, 4, 0, true);
                let next = decode(&fixture.runtime.block_on(pending).unwrap()).unwrap();
                assert_eq!(
                    next["cursor"], "4-0:true:2:true",
                    "coalesces changes and ends with the view"
                );
                let pending = module
                    .begin("nextChange", &args(json!([next["cursor"], "package"])))
                    .unwrap();
                host.close();
                assert!(
                    fixture.runtime.block_on(pending).is_err(),
                    "coalesces changes and ends with the view"
                );
                assert!(
                    module.begin("nextChange", &args(json!([]))).is_err(),
                    "coalesces changes and ends with the view"
                );
                fixture.close();
            }) as fn(&str),
        ),
        (
            "hidden_controllers_wait_and_catch_up_once",
            (|_scenario: &str| {
                let fixture = Fixture::new(true);
                fixture.package();
                let package = fixture.install(0);
                for _ in 0..3 {
                    let host = Host::new(
                        fixture.binding.client.clone(),
                        Context {
                            invocation: None,
                            turn: None,
                            surface: Default::default(),
                            package: package.summary.reference(),
                            worktree: Some(fixture.session.worktree),
                            session: Some(fixture.session.id),
                        },
                        fixture.runtime.handle().clone(),
                        false,
                        None,
                    );
                    host.observe(1, 1, 1, true);
                    host.set_visible(false);
                    let module = host.sdk();
                    let initial = complete(&fixture, &module, "nextChange", json!(["", "settled"]));
                    assert_eq!(
                        initial["visible"], false,
                        "hidden controllers wait and catch up once"
                    );
                    assert_eq!(
                        host.changes.waiting(),
                        0,
                        "hidden controllers wait and catch up once"
                    );
                    let pending = module
                        .begin("nextChange", &args(json!([initial["cursor"], "settled"])))
                        .unwrap();
                    let shown = fixture.runtime.block_on(async {
                        let mut pending = std::pin::pin!(pending);
                        for cursor in 2..50 {
                            host.observe(cursor, 1, cursor, true);
                            assert!(
                                futures::poll!(&mut pending).is_pending(),
                                "hidden controllers wait and catch up once"
                            );
                            assert_eq!(
                                host.changes.waiting(),
                                1,
                                "hidden controllers wait and catch up once"
                            );
                        }
                        host.set_visible(true);
                        decode(&pending.await.unwrap()).unwrap()
                    });
                    assert_eq!(
                        shown["visible"], true,
                        "hidden controllers wait and catch up once"
                    );
                    assert_eq!(
                        host.changes.waiting(),
                        0,
                        "hidden controllers wait and catch up once"
                    );
                    let pending = module
                        .begin("nextChange", &args(json!([shown["cursor"], "settled"])))
                        .unwrap();
                    fixture.runtime.block_on(async {
                        let mut pending = std::pin::pin!(pending);
                        assert!(
                            futures::poll!(&mut pending).is_pending(),
                            "hidden controllers wait and catch up once"
                        );
                        // Node-only traffic cannot reawaken a settled controller.
                        host.observe(100, 1, 49, true);
                        assert!(
                            futures::poll!(&mut pending).is_pending(),
                            "hidden controllers wait and catch up once"
                        );
                        host.close();
                        assert!(
                            pending.await.is_err(),
                            "hidden controllers wait and catch up once"
                        );
                    });
                    assert_eq!(
                        host.changes.waiting(),
                        0,
                        "hidden controllers wait and catch up once"
                    );
                }
                fixture.close();
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
