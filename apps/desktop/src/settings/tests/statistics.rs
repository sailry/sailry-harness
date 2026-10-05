use super::*;
use crate::settings::{
    plugins::test_support::{draw, shown, tap, wait},
    providers::fixture::Fixture,
};
use sailry_client::Client;
use sailry_protocol::{Command, Effort, Output, Permission, SessionConfig, WorkMode};

fn populate(fixture: &Fixture, index: usize, endpoint: &str) {
    let client = Client::new(fixture.transports[index].clone());
    let execute = |command| {
        fixture
            .runtime
            .block_on(client.execute(client.prepare(command)))
            .unwrap()
    };
    let mut provider = fixture.providers[index].clone();
    let revision = provider.revision;
    provider.endpoint = endpoint.into();
    execute(Command::PutProvider {
        provider: provider.clone(),
        expected_revision: revision,
    });
    let Output::Session(session) = execute(Command::CreateSession {
        project: None,
        worktree: None,
        config: Some(SessionConfig {
            assistant: None,
            resource: None,
            provider: provider.id,
            model: provider.default_model,
            effort: Effort::Default,
            mode: WorkMode::Code,
            permission: Permission::Ask,
            credential: None,
        }),
    }) else {
        panic!("session expected");
    };
    execute(Command::SubmitTurn {
        session: session.id,
        expected_revision: session.revision,
        message: "Count this response".into(),
    });
    let now = chrono::Utc::now().timestamp_millis();
    let query = sailry_protocol::usage::Query {
        start_ms: now - 86400000,
        end_ms: now + 86400000,
        dimension: sailry_protocol::usage::Dimension::Model,
        projects: vec![],
        worktrees: vec![],
        providers: vec![],
        models: vec![],
        before: None,
    };
    fixture.runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let report = client.read_usage(query.clone()).await.unwrap();
                if report.totals.responses == 1 {
                    assert_eq!(report.totals.tokens.unwrap().input, 12);
                    assert_eq!(report.requests.items.len(), 1);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("persisted usage deadline");
    });
}

#[gpui::test]
fn observes_each_host_and_keeps_filters_in_the_header(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("en");
    let fixture = Fixture::new();
    let server = fixture
        .runtime
        .block_on(crate::agent_fixture::Server::tools(vec![]));
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(crate::preferences::Preferences::open(
            fixture.directory.path().join("preferences.json"),
        ));
        cx.set_global(crate::backend::Services {
            runtime: fixture.runtime.clone(),
            local: fixture.nodes[0].local(),
            link: fixture.nodes[0].link(),
            relay_enabled: false,
        });
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
        owner = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let owner = owner.unwrap();
    wait(visual, |cx| {
        fixture.nodes.iter().all(|node| {
            owner
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&node.id())
        })
    });
    for index in 0..2 {
        owner.update(visual, |owner, cx| {
            owner
                .live
                .as_mut()
                .unwrap()
                .select(fixture.nodes[index].id(), cx);
        });
        wait(visual, |cx| {
            owner.read(cx).extension_entries(cx).iter().any(|entry| {
                entry.node == fixture.nodes[index].id() && entry.package.name == "statistics"
            })
        });
        let entry = owner.read_with(visual, |owner, cx| {
            owner
                .extension_entries(cx)
                .into_iter()
                .find(|entry| {
                    entry.node == fixture.nodes[index].id() && entry.package.name == "statistics"
                })
                .unwrap()
        });
        assert_eq!(
            entry.navigation.surface,
            sailry_protocol::plugin::desktop::Surface::Workspace
        );
        tap(visual, "navigation-more");
        tap(
            visual,
            Box::leak(format!("more-{}", entry.selector()).into_boxed_str()),
        );
        wait(visual, |cx| {
            owner
                .read(cx)
                .extensions
                .as_ref()
                .and_then(|state| state.panel.as_ref())
                .is_some_and(|panel| {
                    crate::plugins::diagnostics(panel, cx).contains("usage-summary")
                })
        });
        let panel = owner.read_with(visual, |owner, _| {
            owner.extensions.as_ref().unwrap().panel.clone().unwrap()
        });
        shown(visual, "empty-usage-requests-empty", true);
        shown(visual, "empty-usage-ranking-empty", true);
        let table = visual.debug_bounds("usage-requests").unwrap();
        let header = visual.debug_bounds("usage-requests-header").unwrap();
        let empty = visual.debug_bounds("empty-usage-requests-empty").unwrap();
        assert_eq!(header.size.height, px(48.));
        assert_eq!(empty.top(), header.bottom());
        assert_eq!(empty.bottom(), table.bottom());
        for column in 0..6 {
            shown(
                visual,
                Box::leak(format!("usage-requests-header-{column}").into_boxed_str()),
                true,
            );
        }
        let ranking_icon = visual
            .debug_bounds("empty-icon-usage-ranking-empty")
            .unwrap();
        let ranking_title = visual
            .debug_bounds("empty-title-usage-ranking-empty")
            .unwrap();
        let icon = visual
            .debug_bounds("empty-icon-usage-requests-empty")
            .unwrap();
        let title = visual
            .debug_bounds("empty-title-usage-requests-empty")
            .unwrap();
        assert_eq!(icon.size, ranking_icon.size);
        assert_eq!(title.size, ranking_title.size);
        assert!(icon.top() >= empty.top() && title.bottom() <= empty.bottom());
        assert_eq!(title.top() - icon.bottom(), px(12.));
        assert!((icon.center().x - title.center().x).abs() < px(1.));
        assert!((icon.center().x - empty.center().x).abs() < px(1.));
        assert!(((icon.top() + title.bottom()) / 2. - empty.center().y).abs() < px(1.));
        shown(visual, "usage-requests-row-0", false);
        populate(&fixture, index, &server.endpoint);
        wait(visual, |cx| {
            crate::plugins::diagnostics(&panel, cx).contains("12 · 4")
        });
        shown(visual, "empty-usage-requests-empty", false);
        shown(visual, "empty-usage-ranking-empty", false);
        shown(visual, "usage-requests-cell-0-2-secondary", true);
        assert_eq!(
            visual
                .debug_bounds("usage-requests-row-0")
                .unwrap()
                .size
                .height,
            px(48.)
        );
        let row = visual.debug_bounds("usage-requests-row-0").unwrap();
        let first = visual
            .debug_bounds("usage-requests-cell-0-2-primary")
            .unwrap();
        let second = visual
            .debug_bounds("usage-requests-cell-0-2-secondary")
            .unwrap();
        assert!(
            first.top() >= row.top() + px(4.)
                && first.bottom() + px(4.) <= second.top()
                && second.bottom() + px(4.) <= row.bottom()
        );
        assert!(panel.read_with(visual, |_, cx| {
            crate::plugins::diagnostics(&panel, cx).contains("12 · 4")
        }));
        assert_eq!(
            owner.read_with(visual, |owner, _| owner.page),
            crate::preview::Page::Plugin
        );
        assert!(
            visual.debug_bounds("activity-host-filter").is_none(),
            "Usage uses the Shell host switch"
        );
        assert!(visual.debug_bounds("usage-filter-host").is_none());
        wait(visual, |cx| {
            panel.read(cx).header().is_some_and(|header| {
                let providers = header.read(cx).filter_items("usage-filter-provider");
                providers.contains(&fixture.providers[index].name)
                    && !providers.contains(&fixture.providers[1 - index].name)
            })
        });
        assert!(visual.debug_bounds("settings-heading").is_none());
        for width in [1280., 760., 1280.] {
            visual.simulate_resize(size(px(width), px(1000.)));
            // Header fitting is deferred from prepaint to the next render.
            shown(visual, "usage-filter-model", width != 760.);
            let header = visual.debug_bounds("shell-module-header").unwrap();
            let controls = visual.debug_bounds("plugin-header-filters").unwrap();
            assert!(controls.top() >= header.top() && controls.bottom() <= header.bottom());
            let field = visual.debug_bounds("usage-filter-range").unwrap();
            assert!(field.left() >= header.left() && field.right() <= header.right());
            if width == 760. {
                assert!(visual.debug_bounds("usage-filter-model").is_none());
                tap(visual, "plugin-header-filters-trigger");
                shown(visual, "usage-filter-model", true);
                visual.simulate_keystrokes("escape");
                draw(visual);
            } else {
                assert!(visual.debug_bounds("usage-filter-model").is_some());
            }
        }
        let original = panel.entity_id();
        let released = panel.downgrade();
        drop(panel);
        for target in [1 - index, index] {
            tap(visual, "sidebar-host");
            tap(
                visual,
                Box::leak(
                    format!(
                        "sidebar-host-option-{}",
                        crate::live::node_key(fixture.nodes[target].id())
                    )
                    .into_boxed_str(),
                ),
            );
            wait(visual, |cx| {
                let shell = owner.read(cx);
                shell.live.as_ref().unwrap().selected == fixture.nodes[target].id()
                    && shell.extensions.as_ref().unwrap().panel.is_none()
                    && released.upgrade().is_none()
            });
        }
        assert!(released.upgrade().is_none());
        tap(visual, "navigation-more");
        tap(
            visual,
            Box::leak(format!("more-{}", entry.selector()).into_boxed_str()),
        );
        let current = owner.read_with(visual, |owner, _| {
            owner.extensions.as_ref().unwrap().panel.clone().unwrap()
        });
        assert_ne!(
            original,
            current.entity_id(),
            "Switching the Shell host releases the old observer and view"
        );
        let panel = current;
        wait(visual, |cx| {
            crate::plugins::diagnostics(&panel, cx).contains("usage-summary")
                && panel.read(cx).header().is_some_and(|header| {
                    let providers = header.read(cx).filter_items("usage-filter-provider");
                    providers.contains(&fixture.providers[index].name)
                        && !providers.contains(&fixture.providers[1 - index].name)
                })
        });
        let client = Client::new(fixture.transports[index].clone());
        let Output::Plugin(info) = fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::ReadPlugin {
                name: "statistics".into(),
            })))
            .unwrap()
        else {
            panic!("package expected")
        };
        fixture
            .runtime
            .block_on(client.execute(client.prepare(Command::SetPluginEnabled {
                name: "statistics".into(),
                expected_revision: info.summary.revision,
                enabled: false,
            })))
            .unwrap();
        wait(visual, |cx| {
            owner
                .read(cx)
                .extensions
                .as_ref()
                .unwrap()
                .metadata
                .read(cx)
                .entries
                .get("statistics")
                .is_some_and(|package| !package.summary.enabled)
        });
        wait(visual, |cx| {
            // Disabled workspace panels are intentionally unavailable, not healthy snapshots.
            !panel.read(cx).resource_active() && panel.read(cx).header().is_none()
        });
        shown(visual, "usage-summary", false);
        shown(visual, "plugin-header-filters", false);
        visual.update(|window, cx| {
            owner.update(cx, |owner, cx| {
                owner.navigate(crate::preview::Page::Host, window, cx)
            });
        });
        draw(visual);
        assert!(visual.debug_bounds("plugin-header-filters").is_none());
    }
    visual.update(|window, cx| {
        owner.update(cx, |owner, cx| {
            owner.navigate(crate::preview::Page::Settings, window, cx)
        });
    });
    wait(visual, |cx| {
        owner
            .read(cx)
            .settings
            .read(cx)
            .plugin_catalog
            .metadata
            .as_ref()
            .is_some_and(|metadata| metadata.read(cx).settled())
    });
    assert!(owner.read_with(visual, |owner, cx| {
        owner
            .settings
            .read(cx)
            .plugin_settings_entries(cx)
            .iter()
            .all(|entry| entry.name != "statistics")
    }));
    shown(visual, "plugin-settings-statistics", false);
    visual.update(|window, _| window.remove_window());
    drop(owner);
    fixture.close();
}
