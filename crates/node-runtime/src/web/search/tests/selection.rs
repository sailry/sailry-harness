use super::*;

fn configured() -> Plan {
    let mut plan = plan(&Driver::ALL);
    plan.settings.driver = Some(Selection::Auto);
    plan.settings
        .providers
        .get_mut(&Driver::Model)
        .unwrap()
        .model = Some(search::ModelConfig {
        provider: sailry_protocol::ProviderId::new(),
        model: "search-model".into(),
    });
    plan
}

#[test]
fn preserves_search_contracts() {
    for (scenario, check) in [
        (
            "follows_saved_order_in_auto",
            (|scenario: &'static str| {
                let mut plan = configured();
                assert_eq!(candidates(&plan.settings), Driver::ALL, "{scenario}");
                plan.settings.order.reverse();
                assert_eq!(
                    candidates(&plan.settings),
                    plan.settings.order,
                    "{scenario}"
                );
                plan.settings
                    .providers
                    .get_mut(&Driver::Tinyfish)
                    .unwrap()
                    .enabled = false;
                plan.settings
                    .providers
                    .get_mut(&Driver::Brave)
                    .unwrap()
                    .key_configured = false;
                plan.settings
                    .providers
                    .get_mut(&Driver::Model)
                    .unwrap()
                    .model = None;
                assert_eq!(
                    candidates(&plan.settings),
                    [
                        Driver::Deepseek,
                        Driver::Volcengine,
                        Driver::Bocha,
                        Driver::Searxng,
                        Driver::Tavily,
                        Driver::Exa
                    ],
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "preserves_manual_selection",
            (|scenario: &'static str| {
                let mut plan = configured();
                plan.settings.order.reverse();
                plan.settings.driver = Some(Selection::Driver(Driver::Exa));
                let drivers = candidates(&plan.settings);
                assert_eq!(drivers[0], Driver::Exa, "{scenario}");
                assert_eq!(drivers[1], Driver::Deepseek, "{scenario}");
                assert!(!drivers.contains(&Driver::Model), "{scenario}");
                plan.settings.driver = Some(Selection::Driver(Driver::Model));
                assert_eq!(candidates(&plan.settings), [Driver::Model], "{scenario}");
                plan.settings.driver = Some(Selection::Driver(Driver::Tavily));
                plan.settings.fallback = false;
                assert_eq!(candidates(&plan.settings), [Driver::Tavily], "{scenario}");
            }),
        ),
        (
            "automatic_advances_past_failures",
            (|scenario: &'static str| {
                let unsupported = model::failure(Fault::invalid("no web search metadata"));
                assert!(unsupported.recoverable(true), "{scenario}");
                assert!(!unsupported.recoverable(false), "{scenario}");
                assert!(Failure::unauthorized().recoverable(true), "{scenario}");
                assert!(
                    Failure::unavailable("service unavailable").recoverable(false),
                    "{scenario}"
                );
                assert!(!Failure::cancelled().recoverable(true), "{scenario}");
                assert!(
                    !Failure::cleanup_uncertain().recoverable(true),
                    "{scenario}"
                );
                assert!(
                    !Failure::deadline_with_cleanup(false).recoverable(true),
                    "{scenario}"
                );
                assert!(
                    !Failure::unauthorized()
                        .with_cleanup_uncertain()
                        .recoverable(true),
                    "{scenario}"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
