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
fn follows_saved_order_in_auto() {
    let mut plan = configured();
    assert_eq!(candidates(&plan.settings), Driver::ALL);
    plan.settings.order.reverse();
    assert_eq!(candidates(&plan.settings), plan.settings.order);
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
        ]
    );
}

#[test]
fn preserves_manual_selection() {
    let mut plan = configured();
    plan.settings.order.reverse();
    plan.settings.driver = Some(Selection::Driver(Driver::Exa));
    let drivers = candidates(&plan.settings);
    assert_eq!(drivers[0], Driver::Exa);
    assert_eq!(drivers[1], Driver::Deepseek);
    assert!(!drivers.contains(&Driver::Model));
    plan.settings.driver = Some(Selection::Driver(Driver::Model));
    assert_eq!(candidates(&plan.settings), [Driver::Model]);
    plan.settings.driver = Some(Selection::Driver(Driver::Tavily));
    plan.settings.fallback = false;
    assert_eq!(candidates(&plan.settings), [Driver::Tavily]);
}

#[test]
fn automatic_advances_past_failures() {
    let unsupported = model::failure(Fault::invalid("no web search metadata"));
    assert!(unsupported.recoverable(true));
    assert!(!unsupported.recoverable(false));
    assert!(Failure::unauthorized().recoverable(true));
    assert!(Failure::unavailable("service unavailable").recoverable(false));
    assert!(!Failure::cancelled().recoverable(true));
    assert!(!Failure::cleanup_uncertain().recoverable(true));
    assert!(!Failure::deadline_with_cleanup(false).recoverable(true));
    assert!(
        !Failure::unauthorized()
            .with_cleanup_uncertain()
            .recoverable(true)
    );
}
