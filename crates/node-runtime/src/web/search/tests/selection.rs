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
fn keeps_permission_and_cancellation_terminal() {
    assert!(!model::can_fallback(&Failure::cancelled()));
    assert!(!model::can_fallback(&model::failure(Fault::new(
        ErrorCode::PermissionDenied,
        "source address is not allowed",
    ))));
    assert!(!model::can_fallback(&model::failure(Fault::invalid(
        "invalid request"
    ))));
    assert!(model::can_fallback(&Failure::unavailable(
        "service unavailable"
    )));
}
