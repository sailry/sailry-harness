//! Application bootstrap; feature modules remain owned by the crate root.
#[cfg(feature = "workload-tests")]
use crate::workload;
use crate::{assets, backend, locale, oauth, shell, startup, theme, tr, updater};
#[cfg(target_os = "macos")]
use crate::{computer_worker, macos};
use gpui_kit::component::Root;
use gpui_kit::{
    App, AppContext as _, Styled as _, TitlebarOptions, WindowBackgroundAppearance, WindowBounds,
    WindowOptions, point, px, size, transparent_black,
};

#[cfg(not(all(test, feature = "design-capture")))]
pub(crate) fn launch() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    if updater::run_helper() {
        return;
    }
    #[cfg(target_os = "macos")]
    if computer_worker::run_if_requested() {
        return;
    }
    #[cfg(feature = "workload-tests")]
    {
        workload::install_exit();
        let output =
            std::env::var_os("SAILRY_WORKLOAD_OUTPUT").expect("workload output directory required");
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        let _ = workload::setup(|| {
            workload::run(&output);
        });
        if workload::failed() {
            std::process::exit(1);
        }
    }
    #[cfg(not(feature = "workload-tests"))]
    run();
}

pub(crate) fn run(#[cfg(feature = "workload-tests")] workload: workload::Scenario) {
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
    let logging = match &launch {
        startup::Launch::Node(options) => sailry_node_runtime::logging::install(
            &options.data_dir,
            sailry_node_runtime::logging::Application::Desktop,
        )
        .map(Some),
        startup::Launch::Preview => sailry_node_runtime::logging::preview().map(Some),
        startup::Launch::CheckTheme(_) => Ok(None),
    };
    let logging_failed = logging.is_err();
    if logging_failed {
        eprintln!("sailry-desktop: logging is unavailable");
    }
    let _logging = logging.ok().flatten();
    let backend = match launch {
        startup::Launch::CheckTheme(path) => {
            match theme::check::run(&path) {
                Ok(report) => println!("{report}"),
                Err(error) => {
                    eprintln!("sailry-desktop: {error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        startup::Launch::Preview => None,
        startup::Launch::Node(options) => match backend::Owner::start(options) {
            Ok(owner) => Some(owner),
            Err(error) => {
                eprintln!("sailry-desktop: {error}");
                std::process::exit(1);
            }
        },
    };
    let application = gpui_kit::application().with_assets(assets::Assets);
    let returns = oauth::listen(&application);
    application.run(move |cx| {
        cx.set_app_identity("ai.sailry.desktop", "Sailry");
        if let Some(backend) = backend {
            backend.install(cx);
        }
        #[cfg(all(feature = "workload-tests", target_os = "macos"))]
        workload::native_overlays::init(cx);
        gpui_kit::init(cx);
        theme::init(cx);
        locale::init(cx);
        oauth::init(returns, cx);
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
            let opening = async {
                let window = cx
                    .open_window(options, |window, cx| {
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
                if logging_failed {
                    _ = window.update(cx, |_, window, cx| {
                        crate::feedback::error("", &crate::tr("logs_unavailable"), window, cx);
                    });
                }
                cx.update(|cx| cx.activate(true));
                cx.update(|cx| {
                    if cx.try_global::<backend::Services>().is_some() {
                        println!("Desktop window ready");
                    } else {
                        println!("Preview window ready");
                    }
                });
            };
            #[cfg(feature = "workload-tests")]
            if workload::check(opening).await.is_none() {
                cx.update(|cx| cx.quit());
            }
            #[cfg(not(feature = "workload-tests"))]
            opening.await;
        })
        .detach();
    });
}

pub(crate) fn window_options(cx: &App) -> WindowOptions {
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
