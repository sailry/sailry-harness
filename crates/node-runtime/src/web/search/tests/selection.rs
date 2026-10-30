use super::*;

#[test]
fn recognizes_a_han_majority() {
    for query in ["今天的新闻", "最新 Rust 版本发布", "中文，消息！", "𠀀新闻"]
    {
        assert!(chinese(query), "{query}");
    }
    for query in [
        "Rust release",
        "Rust 中文",
        "最新 Rust 发布",
        "中a",
        "123 新闻",
        "🙂!?",
        "",
    ] {
        assert!(!chinese(query), "{query}");
    }
}

#[test]
fn prefers_domestic_drivers_only_in_auto() {
    let mut plan = plan(&Driver::ALL);
    plan.settings.driver = Some(Selection::Auto);
    let drivers = ordered(&plan.settings, "今天新闻");
    assert_eq!(&drivers[..2], &[Driver::Bocha, Driver::Volcengine]);
    assert_eq!(
        &drivers[2..],
        &[
            Driver::Exa,
            Driver::Tinyfish,
            Driver::Tavily,
            Driver::Brave,
            Driver::Searxng,
            Driver::Deepseek
        ]
    );
    assert_eq!(
        ordered(&plan.settings, "current news"),
        Driver::ALL
            .into_iter()
            .filter(|driver| *driver != Driver::Model)
            .collect::<Vec<_>>()
    );
    plan.settings.driver = Some(Selection::Driver(Driver::Exa));
    assert_eq!(ordered(&plan.settings, "今天新闻")[0], Driver::Exa);
}

#[test]
fn keeps_native_search_explicit() {
    let mut plan = plan(&Driver::ALL);
    plan.settings
        .providers
        .get_mut(&Driver::Model)
        .unwrap()
        .model = Some(search::ModelConfig {
        provider: sailry_protocol::ProviderId::new(),
        model: "search-model".into(),
    });
    for selection in [Selection::Auto, Selection::Driver(Driver::Exa)] {
        plan.settings.driver = Some(selection);
        assert!(!candidates(&plan.settings).contains(&Driver::Model));
    }
    plan.settings.driver = Some(Selection::Driver(Driver::Model));
    assert!(plan.settings.fallback);
    assert_eq!(candidates(&plan.settings), [Driver::Model]);
}
