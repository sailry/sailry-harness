use super::*;
use crate::settings::{
    plugins::test_support::{draw, shown, shown_settings, tap, wait},
    providers::fixture::Fixture,
};
use sailry_client::Client;
use sailry_protocol::{
    Command, Output,
    media::{Binding, Kind, Settings},
};

fn execute(fixture: &Fixture, index: usize, command: Command) -> Output {
    let client = Client::new(fixture.transports[index].clone());
    fixture
        .runtime
        .block_on(client.execute(client.prepare(command)))
        .unwrap()
}

fn read(fixture: &Fixture, index: usize) -> Settings {
    let Output::MediaSettings(settings) = execute(fixture, index, Command::ReadMediaSettings)
    else {
        panic!("media settings expected")
    };
    settings
}

fn ready_menu(owner: &Entity<Workspace>, visual: &mut VisualTestContext, selector: &str) {
    wait(visual, |cx| {
        owner.read(cx).plugin_settings_panel().is_some_and(|panel| {
            crate::plugins::diagnostics(&panel, cx).lines().any(|line| {
                line.contains("sailry/ui.SelectField")
                    && line.contains(selector)
                    && line.contains("(\"disabled\", Bool(false))")
            })
        })
    });
}

fn select(visual: &mut VisualTestContext, selector: &'static str, index: usize) {
    tap(visual, selector);
    // The retained Select cursor starts at the currently configured option.
    visual.simulate_keystrokes(if index == 0 { "up enter" } else { "down enter" });
    draw(visual);
}

#[gpui::test]
fn saves_clears_and_isolates_hosts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let fixture = Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(crate::preferences::Preferences::open(
            fixture.directory.path().join("preferences.json"),
        ));
    });
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| crate::shell::Shell::new(window, cx));
        shell.update(cx, |shell, cx| {
            shell.navigate(crate::preview::Page::Settings, window, cx)
        });
        owner = Some(shell.read(cx).settings.clone());
        Root::new(shell, window, cx)
    });
    let owner = owner.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1280.), px(1050.)));
    let mut prior: Option<WeakEntity<crate::plugins::Panel>> = None;
    for index in 0..2 {
        owner.update(visual, |owner, cx| {
            owner.bind_providers(
                fixture.transports[index].clone(),
                fixture.runtime.clone(),
                format!("Host {index}").into(),
                cx,
            )
        });
        shown(visual, "plugin-settings-media", true);
        let providers = visual.debug_bounds("settings_providers").unwrap();
        let media = visual.debug_bounds("plugin-settings-media").unwrap();
        let delegation = visual.debug_bounds("plugin-settings-delegation").unwrap();
        assert!(providers.bottom() <= media.top() && media.bottom() <= delegation.top());
        tap(visual, "plugin-settings-media");
        shown_settings(&owner, visual, "media-settings", true);
        for selector in [
            "settings-control-media_image_understanding",
            "settings-control-media_image_generation",
            "settings-control-media_video_generation",
        ] {
            shown_settings(&owner, visual, selector, true);
        }
        for selector in [
            "settings-control-media_audio_transcription",
            "settings-control-media_speech_synthesis",
            "settings-control-media_video_understanding",
        ] {
            shown_settings(&owner, visual, selector, false);
        }
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_settings_panel()
                .is_some_and(|panel| panel.read(cx).resource_active())
        });
        owner.read_with(visual, |owner, cx| {
            let panel = owner.plugin_settings_panel().unwrap();
            assert_eq!(owner.section, Section::Plugin);
            assert_eq!(owner.selected_plugin_settings(), Some("media"));
            if let Some(prior) = &prior {
                assert_ne!(&panel.downgrade(), prior);
            }
            assert!(panel.read(cx).resource_active());
        });
        if let Some(prior) = &prior {
            wait(visual, |cx| {
                prior
                    .upgrade()
                    .is_none_or(|panel| !panel.read(cx).resource_active())
            });
        }
        assert!(read(&fixture, index).bindings.is_empty());
        ready_menu(&owner, visual, "media_image_understanding");
        select(visual, "media_image_understanding", 1);
        wait(visual, |_| read(&fixture, index).revision == 1);
        let binding = Binding {
            provider: fixture.providers[index].id,
            model: "vision".into(),
        };
        assert_eq!(read(&fixture, index).bindings[&Kind::Vision], binding);

        // An already-open select must reject a choice from a stale Node revision.
        ready_menu(&owner, visual, "media_image_generation");
        tap(visual, "media_image_generation");
        let settings = read(&fixture, index);
        execute(&fixture, index, Command::SaveMediaSettings(settings));
        visual.simulate_keystrokes("down enter");
        wait(visual, |_| read(&fixture, index).revision == 2);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            draw(visual);
            if visual.update(|window, cx| !window.notifications(cx).is_empty()) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "media conflict notification deadline"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!read(&fixture, index).bindings.contains_key(&Kind::Image));

        ready_menu(&owner, visual, "media_image_generation");
        select(visual, "media_image_generation", 1);
        wait(visual, |_| read(&fixture, index).revision == 3);
        assert_eq!(
            read(&fixture, index).bindings[&Kind::Image],
            Binding {
                provider: fixture.providers[index].id,
                model: "image".into()
            }
        );
        ready_menu(&owner, visual, "media_image_generation");
        select(visual, "media_image_generation", 0);
        wait(visual, |_| read(&fixture, index).revision == 4);
        assert_eq!(
            read(&fixture, index).bindings,
            [(Kind::Vision, binding)].into_iter().collect()
        );
        ready_menu(&owner, visual, "media_image_understanding");
        select(visual, "media_image_understanding", 0);
        wait(visual, |_| read(&fixture, index).revision == 5);
        assert!(read(&fixture, index).bindings.is_empty());
        let Output::Plugins(packages) = execute(&fixture, index, Command::ListPlugins) else {
            panic!("plugins expected")
        };
        let package = packages
            .into_iter()
            .find(|package| package.name == "media")
            .unwrap();
        assert!(package.enabled);
        execute(
            &fixture,
            index,
            Command::SetPluginEnabled {
                name: package.name,
                expected_revision: package.revision,
                enabled: false,
            },
        );
        wait(visual, |cx| {
            owner
                .read(cx)
                .plugin_catalog
                .packages
                .iter()
                .any(|package| package.name == "media" && !package.enabled)
        });
        shown_settings(&owner, visual, "media-settings", true);
        ready_menu(&owner, visual, "media_image_understanding");
        select(visual, "media_image_understanding", 1);
        wait(visual, |_| read(&fixture, index).revision == 6);
        assert_eq!(
            read(&fixture, index).bindings[&Kind::Vision].provider,
            fixture.providers[index].id
        );
        ready_menu(&owner, visual, "media_image_understanding");
        select(visual, "media_image_understanding", 0);
        wait(visual, |_| read(&fixture, index).revision == 7);
        prior = Some(owner.read_with(visual, |owner, _| {
            owner.plugin_settings_panel().unwrap().downgrade()
        }));
        draw(visual);
    }
    let Output::Plugins(packages) = execute(&fixture, 1, Command::ListPlugins) else {
        panic!("plugins expected")
    };
    let package = packages
        .into_iter()
        .find(|package| package.name == "media")
        .unwrap();
    execute(
        &fixture,
        1,
        Command::RemovePlugin {
            name: package.name,
            expected_revision: package.revision,
        },
    );
    shown(visual, "plugin-settings-media", false);
    wait(visual, |cx| {
        prior
            .as_ref()
            .unwrap()
            .upgrade()
            .is_none_or(|panel| !panel.read(cx).resource_active())
    });
    visual.update(|window, _| window.remove_window());
    drop(owner);
    fixture.close();
}
