use super::process_actions::{Dispatch, Operation};
use super::*;
use core::prelude::v1::test;
use sailry_node_runtime::Node;
use sailry_protocol::host::metrics::Process;
use std::time::{Duration, Instant};

struct Frame(Entity<Monitor>);

impl Render for Frame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

#[gpui::test]
fn process_confirmations(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    // Keep the established single-line label baseline independent of other tests.
    rust_i18n::set_locale("zh-CN");
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    let invitation = remote.link().invite().unwrap();
    let address = runtime
        .block_on(local.link().pair(invitation.ticket()))
        .unwrap();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_reduce_motion(true);
    });
    for (node, transport) in [
        (&local, local.local()),
        (&remote, local.link().remote(address)),
    ] {
        let mut monitor = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Monitor::new(transport, runtime.clone(), window, cx));
            monitor = Some(view.clone());
            Root::new(cx.new(|_| Frame(view)), window, cx)
        });
        let monitor = monitor.unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while !monitor.read_with(visual, |view, _| view.view.sample.is_some()) {
            visual.run_until_parked();
            assert!(Instant::now() < deadline, "host sample deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        visual.update(|window, cx| {
            monitor.update(cx, |view, cx| {
                let mut sample = (**view.view.sample.as_ref().unwrap()).clone();
                sample.disks = ["/", "/fixture"]
                    .into_iter()
                    .map(|mount| sailry_protocol::host::metrics::Disk {
                        mount: mount.into(),
                        name: mount.into(),
                        total_bytes: 4096,
                        available_bytes: 2048,
                        io: None,
                    })
                    .collect();
                let mut snapshot = view.view.clone();
                snapshot.sample = Some(Arc::new(sample));
                view.accept(snapshot, cx);
            });
            window.draw(cx).clear(cx);
        });
        let healthy = monitor.read_with(visual, |view, _| view.view.clone());
        let feedback_count = visual.update(|window, cx| window.notifications(cx).len());
        for _ in 0..3 {
            visual.update(|window, cx| {
                monitor.update(cx, |view, cx| {
                    let mut failed = healthy.clone();
                    failed.error = Some(sailry_protocol::Fault::new(
                        sailry_protocol::ErrorCode::Unavailable,
                        "Sampling fixture",
                    ));
                    view.accept(failed, cx);
                });
                window.draw(cx).clear(cx);
            });
            assert!(visual.debug_bounds("host-sampling-error").is_some());
            assert_eq!(
                visual.update(|window, cx| window.notifications(cx).len()),
                feedback_count
            );
            visual.update(|window, cx| {
                monitor.update(cx, |view, cx| view.accept(healthy.clone(), cx));
                window.draw(cx).clear(cx);
            });
            assert!(visual.debug_bounds("host-sampling-error").is_none());
        }
        let heading = visual.debug_bounds("host-partition-heading-0").unwrap();
        for selector in ["host-partition-label-0", "host-partition-label-1"] {
            let label = visual.debug_bounds(selector).unwrap();
            assert!(
                (label.left() - heading.left()).abs() <= px(1.),
                "partition paths align with the column heading"
            );
        }
        let partition = visual.debug_bounds("host-partition-1").unwrap();
        visual.simulate_click(partition.center(), Modifiers::default());
        visual.run_until_parked();
        assert_eq!(
            monitor.read_with(visual, |view, _| view.disk.clone()),
            Some("/fixture".into())
        );
        let row = visual.debug_bounds("host-process-row-0").unwrap();
        assert!(
            row.size.height >= px(32.),
            "process rows use regular Kit sizing"
        );
        let process = Process {
            pid: i32::MAX as u32,
            started_at_secs: 1,
            name: "Captured process".into(),
            cpu_basis_points: Some(250),
            memory_bytes: 1024,
        };
        let dispatch = |operation, visual: &mut VisualTestContext| {
            visual.update(|window, cx| {
                monitor.read(cx).focus.clone().focus(window, cx);
                window.dispatch_action(
                    Box::new(Dispatch {
                        node: node.id(),
                        process: process.clone(),
                        operation,
                    }),
                    cx,
                );
            });
            visual.run_until_parked();
        };
        for (operation, expected) in [
            (Operation::CopyPid, process.pid.to_string()),
            (Operation::CopyName, process.name.clone()),
        ] {
            dispatch(operation, visual);
            assert_eq!(
                visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
                expected
            );
        }
        dispatch(Operation::Details, visual);
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let name_label = visual
            .debug_bounds("process-detail-label-host_process_name")
            .unwrap();
        let name_value = visual
            .debug_bounds("process-detail-value-host_process_name")
            .unwrap();
        let pid_value = visual
            .debug_bounds("process-detail-value-host_pid")
            .unwrap();
        assert_eq!(name_label.size.width, px(48.));
        assert_eq!(name_value.left() - name_label.right(), px(12.));
        assert!(pid_value.top() - name_value.bottom() <= px(6.));
        visual.update(|window, cx| window.close_dialog(cx));
        dispatch(Operation::Stop, visual);
        let (_, detail) = crate::prompts::tests::wait(visual);
        assert!(detail.contains(&process.name));
        assert!(detail.contains(&process.pid.to_string()));
        crate::prompts::tests::answer(visual, "settings_cancel");
        let database = rusqlite::Connection::open_with_flags(
            node.profile().join("storage/node.sqlite3"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let count = || {
            database
                .query_row("SELECT count(*) FROM requests", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert_eq!(count(), 0);
        dispatch(Operation::ForceStop, visual);
        crate::prompts::tests::answer(visual, "host_process_force_stop");
        let deadline = Instant::now() + Duration::from_secs(5);
        while count() == 0 || monitor.read_with(visual, |view, _| view.stopping) {
            visual.run_until_parked();
            assert!(Instant::now() < deadline, "process command deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(count(), 1);
        // The target does not exist: confirmation dispatches, but cannot harm a real process.
        let result: String = database
            .query_row("SELECT result FROM requests", [], |row| row.get(0))
            .unwrap();
        assert!(result.contains("not_found"));
        visual.update(|window, _| window.remove_window());
        drop(monitor);
    }
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
