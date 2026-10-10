//! User-requested visual acceptance uses Kit's real Metal renderer and fonts.
//! The labeled fixture changes only catalog input; scene and Shell are production.
use super::*;
use gpui_kit::test::TestWindowExt;
use std::{path::Path, sync::Arc};

fn preview(fixture: &Fixture) {
    let directory = fixture.directory.path().join("project/plaza-package");
    let main = directory.join("dev.sailry.platform/desktop/main.js");
    let names = [
        "Sailry", "Desktop", "Mobile", "Website", "Plugins", "Runtime", "Docs", "Sandbox",
    ];
    let running = [3, 2, 3, 1, 2, 2, 1, 1];
    let mut projects = Vec::new();
    let mut worktrees = Vec::new();
    let mut sessions = Vec::new();
    let mut lanes = serde_json::Map::new();
    for (room, name) in names.iter().enumerate() {
        let project = format!("room-{room}");
        let worktree = format!("tree-{room}");
        projects.push(serde_json::json!({"id":project,"name":name}));
        worktrees.push(serde_json::json!({"id":worktree,"project":project,"path":"/preview"}));
        for seat in 0..6 {
            let id = format!("preview-{room}-{seat}");
            let approval = seat == 5 && [0, 2, 5].contains(&room);
            let active = seat < running[room];
            let lane = if approval {
                "waiting"
            } else if active {
                "running"
            } else {
                "idle"
            };
            lanes.insert(id.clone(), lane.into());
            sessions.push(serde_json::json!({
                "id":id,"project":project,"worktree":worktree,"archived":false,"delegation":null,
                "activity":{"title":format!("Preview {}",seat+1),"queued":0,
                    "waiting":if approval {Some("approval")} else {None},
                    "run":if active || approval {Some(serde_json::json!({"turn":format!("turn-{room}-{seat}"),"status":"running"}))} else {None}}
            }));
        }
    }
    let catalog = serde_json::json!({
        "node":fixture.node.id(),"cursor":"preview","projects":projects,"worktrees":worktrees,
        "sessions":sessions,"session_lanes":lanes,"terminals":[],"terminal_lanes":{},"unread_terminals":[],
    });
    let source = std::fs::read_to_string(&main).unwrap();
    let source = source
        .replace(
            "messages(JSON.parse(context()).locale)",
            "messages('zh-CN')",
        )
        .replace(
            "new State(readActivityCatalog,readActivityPreviews,",
            &format!("new State(async () => ({catalog}),async () => [],"),
        );
    std::fs::write(main, source).unwrap();
    let manifest = directory.join("plugin.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    value["extensions"]["dev.sailry.platform"]["desktop"]["navigation"]["label"] =
        "Task Plaza · Preview".into();
    value["extensions"]["dev.sailry.platform"]["desktop"]["navigation"]["locales"]["zh-CN"] =
        "任务广场 · 设计预览".into();
    std::fs::write(manifest, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    fixture.execute(Command::InstallPlugin {
        worktree: fixture.session.worktree,
        path: "plaza-package".into(),
        name: "task-plaza".into(),
        expected_revision: 1,
    });
}

fn wait_render(cx: &mut HeadlessAppContext, handle: AnyWindowHandle, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.advance_clock(Duration::from_millis(25));
        cx.run_until_parked();
        if cx
            .update_window(handle, |_, window, cx| {
                window.refresh();
                window.render_frame(cx);
                ready(cx)
            })
            .unwrap()
        {
            return;
        }
        assert!(Instant::now() < deadline, "rendering fixture deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn capture(output: &Path, width: f32, height: f32, mode: ThemeMode) {
    let fixture = Fixture::new(false);
    install(&fixture);
    preview(&fixture);
    let mut cx = HeadlessAppContext::with_platform(
        gpui_kit::platform::current_platform(true).text_system(),
        Arc::new(crate::assets::Assets),
        gpui_kit::platform::current_headless_renderer,
    );
    cx.allow_parking();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        Theme::change(mode, None, cx);
        cx.set_global(fixture.services(false));
        cx.set_global(crate::preferences::Preferences::default());
    });
    rust_i18n::set_locale("zh-CN");
    let mut shell = None;
    let handle = cx
        .open_window(size(px(width), px(height)), |window, cx| {
            let view = cx.new(|cx| {
                let mut shell = Shell::new(window, cx);
                shell
                    .live
                    .as_mut()
                    .unwrap()
                    .override_transport(fixture.transport.clone());
                shell
            });
            shell = Some(view.clone());
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap();
    let handle = handle.into();
    let shell = shell.unwrap();
    wait_render(&mut cx, handle, |cx| {
        shell
            .read(cx)
            .extension_entries(cx)
            .iter()
            .any(|entry| entry.package.name == "task-plaza")
    });
    cx.update_window(handle, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            let entry = shell
                .extension_entries(cx)
                .into_iter()
                .find(|entry| entry.package.name == "task-plaza")
                .unwrap();
            shell.open_extension(entry, window, cx);
        })
    })
    .unwrap();
    let panel = cx.update(|cx| {
        shell
            .read(cx)
            .extensions
            .as_ref()
            .unwrap()
            .panel
            .as_ref()
            .unwrap()
            .clone()
    });
    wait_render(&mut cx, handle, |cx| {
        snapshot(&panel, cx).contains("plaza-preview-7-0")
    });
    // Shell initialization follows the system appearance. Apply the requested
    // capture theme after its initial preferences and subscriptions settle.
    cx.update_window(handle, |_, window, cx| {
        Theme::change(mode, Some(window), cx)
    })
    .unwrap();
    // Image decoding is asynchronous; settle all genuine resource loads before capture.
    for _ in 0..40 {
        cx.advance_clock(Duration::from_millis(25));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
        cx.update_window(handle, |_, window, cx| {
            window.refresh();
            window.render_frame(cx)
        })
        .unwrap();
    }
    let image = cx
        .capture_screenshot(handle)
        .expect("real Metal screenshot");
    assert_eq!(cx.update(|cx| cx.theme().is_dark()), mode.is_dark());
    let scale = cx
        .update_window(handle, |_, window, _| window.scale_factor())
        .unwrap();
    assert_eq!(
        image.dimensions(),
        ((width * scale) as u32, (height * scale) as u32)
    );
    assert!(
        image
            .pixels()
            .filter(|pixel| pixel[0] > 150 && pixel[1] > 120 && pixel[2] < 170)
            .count()
            > 10000,
        "campus textures must be decoded and drawn"
    );
    image.save(output).unwrap();
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    drop(panel);
    drop(shell);
    drop(cx);
    fixture.close();
}

#[cfg(target_os = "macos")]
pub(in crate::plugins) fn run() {
    let Some(directory) = std::env::var_os("SAILRY_PLAZA_CAPTURE_DIR") else {
        println!("plaza-render: skipped; set SAILRY_PLAZA_CAPTURE_DIR to capture the design");
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    for (name, width, height, mode) in [
        ("actual-dark.png", 1640., 960., ThemeMode::Dark),
        ("actual-light.png", 1640., 960., ThemeMode::Light),
        ("actual-compact.png", 1000., 760., ThemeMode::Dark),
    ] {
        capture(&directory.join(name), width, height, mode);
    }
}

#[cfg(not(target_os = "macos"))]
pub(in crate::plugins) fn run() {
    println!("plaza-render: skipped; this capture requires the macOS Metal renderer");
}
