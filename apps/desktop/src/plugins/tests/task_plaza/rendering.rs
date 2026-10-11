//! User-requested visual acceptance uses Kit's real Metal renderer and fonts.
//! The labeled fixture changes only catalog input; scene and Shell are production.
use super::*;
use gpui_kit::test::TestWindowExt;
use std::{path::Path, sync::Arc};

fn preview(fixture: &Fixture, selection: &str, sparse: bool) {
    let directory = fixture.directory.path().join("project/plaza-package");
    let main = directory.join("dev.sailry.platform/desktop/main.js");
    let names = [
        "Sailry", "Desktop", "Mobile", "Website", "Plugins", "Runtime", "Docs", "Sandbox",
    ];
    let running = [3, 2, 3, 1, 2, 2, 1, 1];
    let mut projects = Vec::new();
    let mut worktrees = Vec::new();
    let mut sessions = Vec::new();
    let mut previews = Vec::new();
    let mut lanes = serde_json::Map::new();
    for (room, name) in names.iter().enumerate() {
        if sparse && room > 1 {
            break;
        }
        let project = if room == 0 {
            fixture.session.project.unwrap().to_string()
        } else {
            format!("room-{room}")
        };
        let worktree = if room == 0 {
            fixture.session.worktree.to_string()
        } else {
            format!("tree-{room}")
        };
        projects.push(serde_json::json!({"id":project,"name":name}));
        worktrees.push(serde_json::json!({"id":worktree,"project":project,"path":"/preview"}));
        for seat in 0..if sparse {
            4
        } else if room == 0 {
            14
        } else {
            6
        } {
            let id = if room == 0 && seat == 0 {
                fixture.session.id.to_string()
            } else {
                format!("preview-{room}-{seat}")
            };
            let approval = seat == 5 && [0, 2, 5].contains(&room);
            let reply = seat == 5 && room == 1;
            let active = seat < running[room] || room == 0 && seat == 13;
            let lane = if approval || reply {
                "waiting"
            } else if active {
                "running"
            } else {
                match seat % 6 {
                    3 => "completed",
                    4 => "failed",
                    _ => "idle",
                }
            };
            lanes.insert(id.clone(), lane.into());
            let titles = [
                "检查远程主机并确认服务运行状态",
                "优化任务广场的布局与交互",
                "分析项目结构并整理当前进度",
                "修复会话列表中的文字裁切问题",
                "完成插件配置与连接验收",
                "更新项目文件，需要审批",
            ];
            let summaries = [
                "已检查连接配置，正在核对服务日志和运行状态",
                "任务卡片已调整，正在验证统一右侧栏中的会话与审批交互",
                "已整理项目结构，正在检查依赖与构建入口",
                "已定位列表高度问题，下一步验证长标题与多行内容",
                "连接配置已保存",
                "即将更新工作区文件，请确认本次操作",
            ];
            sessions.push(serde_json::json!({
                "id":id,"project":project,"worktree":worktree,"archived":false,"delegation":null,
                "activity":{"title":titles[seat % 6],"queued":0,
                    "waiting":if approval {Some("approval")} else if reply {Some("input")} else {None},
                    "run":if lane != "idle" {Some(serde_json::json!({"turn":format!("turn-{room}-{seat}"),"status":if active || approval || reply {"running"} else {lane}}))} else {None}}
            }));
            previews.push(serde_json::json!({"session":id,
                "turn":if lane != "idle" {Some(format!("turn-{room}-{seat}"))} else {None},
                "text":summaries[seat % 6]}));
        }
    }
    let catalog = serde_json::json!({
        "node":fixture.node.id(),"cursor":"1","projects":projects,"worktrees":worktrees,
        "sessions":sessions,"session_lanes":lanes,"terminals":[],"terminal_lanes":{},"unread_terminals":[],
    });
    let previews = serde_json::Value::Array(previews);
    let source = std::fs::read_to_string(&main).unwrap();
    let source = source
        .replace(
            "messages(JSON.parse(context()).locale)",
            "messages('zh-CN')",
        )
        .replace(
            "new State(readActivityCatalog,readActivityPreviews,",
            &format!("new State(async () => ({catalog}),async () => ({previews}),"),
        );
    let selected = match selection {
        "session" => format!(
            "{{kind:'session',id:'{}',title:'检查远程主机并确认服务运行状态'}}",
            fixture.session.id
        ),
        "room" => format!("{{kind:'room',id:'{}'}}", fixture.session.project.unwrap()),
        _ => String::new(),
    };
    let source = if !selected.is_empty() {
        source.replace(
            "render(cx) {return render(this,cx);}",
            &format!("render(cx) {{if (this.board.visible && !this.captureOpened) {{this.captureOpened = true;this.panelOpen = true;this.selected = {selected};}}return render(this,cx);}}"),
        )
    } else {
        source
    };
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
        expected_digest: None,
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

fn capture(output: &Path, width: f32, height: f32, mode: ThemeMode, selection: &str, sparse: bool) {
    let fixture = Fixture::new(false);
    install(&fixture);
    preview(&fixture, selection, sparse);
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
        snapshot(&panel, cx).contains(&format!("plaza-{}", fixture.session.id))
            && (selection != "session"
                || chat(&panel, cx).is_some_and(|chat| chat.read(cx).connected()))
            && (selection != "room" || snapshot(&panel, cx).contains("正在核对服务日志"))
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
        "workstation artwork must be decoded and drawn"
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
    for (name, width, height, mode, selection, sparse) in [
        (
            "actual-dark.png",
            1640.,
            960.,
            ThemeMode::Dark,
            "room",
            false,
        ),
        (
            "actual-light.png",
            1640.,
            960.,
            ThemeMode::Light,
            "room",
            false,
        ),
        (
            "actual-scene.png",
            1640.,
            960.,
            ThemeMode::Dark,
            "none",
            false,
        ),
        (
            "actual-compact.png",
            1000.,
            760.,
            ThemeMode::Dark,
            "none",
            false,
        ),
        (
            "actual-empty.png",
            1640.,
            960.,
            ThemeMode::Dark,
            "none",
            true,
        ),
        (
            "actual-session.png",
            1640.,
            960.,
            ThemeMode::Dark,
            "session",
            false,
        ),
    ] {
        capture(
            &directory.join(name),
            width,
            height,
            mode,
            selection,
            sparse,
        );
    }
}

#[cfg(not(target_os = "macos"))]
pub(in crate::plugins) fn run() {
    println!("plaza-render: skipped; this capture requires the macOS Metal renderer");
}
