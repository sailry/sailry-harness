mod activity;
#[cfg(any(test, feature = "workload-tests"))]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/agent_support/mod.rs"]
mod agent_fixture;
#[cfg(target_os = "macos")]
mod app_menu;
#[cfg(test)]
use agent_fixture as agent_support;
mod assets;
mod backend;
mod browser;
mod command_picker;
#[cfg(target_os = "macos")]
#[path = "../../../crates/node-runtime/src/computer/worker_host/mod.rs"]
mod computer_worker;
mod content;
mod conversation;
mod dictation;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/discovery_support/mod.rs"]
mod discovery_fixture;
mod downloads;
mod empty_state;
mod feedback;
mod header;
mod hosts;
mod live;
mod locale;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/support/mcp_input.rs"]
mod mcp_fixture;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/support/mcp.rs"]
mod mcp_peer;
mod model_picker;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/agent/providers/native/server.rs"]
mod native_provider_fixture;
mod navigation_tabs;
mod pages;
mod panes;
mod permissions;
mod plugins;
mod ports;
mod preferences;
mod preview;
mod prompts;
#[cfg(test)]
#[allow(dead_code)]
mod provider_fixture;
mod reasoning;
mod resources;
mod settings;
mod shell;
mod shortcuts;
mod sidebar;
#[cfg(test)]
#[allow(dead_code)]
#[path = "../../../crates/node-runtime/tests/ssh/server.rs"]
mod ssh_fixture;
mod startup;
mod terminal;
mod theme;
mod ui;
mod updater;
#[cfg(feature = "workload-tests")]
mod workload;
mod workspace;

use gpui_kit::component::Root;
use gpui_kit::*;

rust_i18n::i18n!("../../locales", fallback = "en");

fn tr(key: &str) -> SharedString {
    rust_i18n::t!(key).to_string().into()
}

fn main() {
    if updater::run_helper() {
        return;
    }
    #[cfg(target_os = "macos")]
    if computer_worker::run_if_requested() {
        return;
    }
    #[cfg(feature = "workload-tests")]
    {
        let output =
            std::env::var_os("SAILRY_WORKLOAD_OUTPUT").expect("workload output directory required");
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        workload::run(&output);
    }
    #[cfg(not(feature = "workload-tests"))]
    run();
}

fn run(#[cfg(feature = "workload-tests")] workload: workload::Scenario) {
    #[cfg(not(feature = "workload-tests"))]
    let launch = match startup::parse(std::env::args_os().skip(1)) {
        Ok(Some(launch)) => launch,
        Ok(None) => return,
        Err(error) => {
            eprintln!("sailry-desktop: {error}");
            std::process::exit(1);
        }
    };
    #[cfg(feature = "workload-tests")]
    let launch = startup::parse(
        [
            std::ffi::OsString::from("--data-dir"),
            workload.profile.clone().into_os_string(),
        ]
        .into_iter(),
    )
    .unwrap()
    .unwrap();
    let backend = match launch {
        startup::Launch::Preview => None,
        startup::Launch::Node(options) => match backend::Owner::start(options) {
            Ok(owner) => Some(owner),
            Err(error) => {
                eprintln!("sailry-desktop: {error}");
                std::process::exit(1);
            }
        },
    };
    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            cx.set_app_identity("ai.sailry.desktop", "Sailry");
            if let Some(backend) = backend {
                backend.install(cx);
            }
            gpui_kit::init(cx);
            theme::init(cx);
            locale::init(cx);
            updater::init(cx);
            shell::init(cx);
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let options = window_options(cx);
            cx.spawn(async move |cx| {
                cx.open_window(options, |window, cx| {
                    #[cfg(target_os = "macos")]
                    macos::configure_backdrop(window);
                    let live = cx.try_global::<backend::Services>().is_some();
                    window.set_window_title(&if live {
                        tr("app").to_string()
                    } else {
                        format!("{} - {}", tr("app"), tr("preview"))
                    });
                    let view = cx.new(|cx| shell::Shell::new(window, cx));
                    #[cfg(feature = "workload-tests")]
                    workload::attach(workload, view.clone(), window, cx);
                    cx.new(|cx| Root::new(view, window, cx).bg(transparent_black()))
                })
                .expect("failed to open desktop window");
                cx.update(|cx| cx.activate(true));
                cx.update(|cx| {
                    if cx.try_global::<backend::Services>().is_some() {
                        println!("Desktop window ready");
                    } else {
                        println!("Preview window ready");
                    }
                });
            })
            .detach();
        });
}

fn window_options(cx: &App) -> WindowOptions {
    use gpui_kit::component::TitleBar;
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1280.), px(820.)), cx)),
        window_min_size: Some(crate::shell::minimum_size()),
        window_background: if cfg!(target_os = "windows") {
            WindowBackgroundAppearance::MicaBackdrop
        } else {
            WindowBackgroundAppearance::Blurred
        },
        titlebar: Some(TitlebarOptions {
            traffic_light_position: Some(point(px(12.), px(16.))),
            ..TitleBar::title_bar_options()
        }),
        ..TitleBar::window_options()
    }
}
