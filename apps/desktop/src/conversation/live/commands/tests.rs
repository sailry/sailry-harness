use super::*;
use crate::conversation::live::tests::{
    fixture::{self, Fixture, tap},
    wait,
};
use core::prelude::v1::test;
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

#[gpui::test]
#[cfg(unix)]
fn dev_server_lifecycle(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"background": true, "command": "node -e '\nconsole.log(Array.from({length:12},(_,i)=>i).join(\"\\n\"));\nconst fs=require(\"fs\"); const server=require(\"http\").createServer((q,r)=>r.end(\"background-http-ready\")); server.listen(0,\"127.0.0.1\",()=>{fs.writeFileSync(\"service.port\",String(server.address().port)); console.log(\"http://localhost:\"+server.address().port+\"/app?q=1\");}); setInterval(()=>console.log(\"service heartbeat\"),200);'"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"background": true, "command": "printf 'second service ready'; while :; do sleep 1; done"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "read_command"),
                    json!({}),
                ),
            ],
        );
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        }) else {
            panic!("session expected")
        };
        fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Run dev server".into(),
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session.clone());
        let ports = visual.update(|window, cx| {
            let ports = cx.new(|cx| {
                crate::ports::Workspace::new(
                    fixture.binding.client.clone(),
                    fixture.runtime.clone(),
                    window,
                    cx,
                )
            });
            view.update(cx, |view, cx| view.bind_ports(ports.clone(), cx));
            ports
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .page
                        .runs
                        .iter()
                        .any(|run| run.status == sailry_protocol::conversation::Status::Completed)
                })
        });
        wait(visual, |cx| {
            view.read(cx).commands.read(cx).state.items.len() == 2
        });
        let Output::Snapshot(snapshot) = fixture.execute(Command::Snapshot) else {
            panic!("snapshot expected")
        };
        let package = snapshot
            .plugins
            .iter()
            .find(|package| package.name == "commands")
            .unwrap();
        assert!(package.enabled);
        fixture.execute(Command::SetPluginEnabled {
            name: package.name.clone(),
            expected_revision: package.revision,
            enabled: false,
        });
        wait(visual, |cx| {
            view.read(cx)
                .node
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot
                        .plugins
                        .iter()
                        .any(|package| package.name == "commands" && !package.enabled)
                })
        });
        // Core output, forwarding, and process controls remain available after disabling the tool.
        let info = view.read_with(visual, |view, cx| {
            view.commands
                .read(cx)
                .state
                .items
                .iter()
                .find(|info| info.command.starts_with("node "))
                .unwrap()
                .clone()
        });
        assert_eq!(info.session, session.id);
        assert_eq!(info.status, CommandStatus::Running);
        wait(visual, |cx| {
            view.read(cx)
                .commands
                .read(cx)
                .state
                .items
                .iter()
                .any(|info| !info.services.is_empty())
        });
        let port = view.read_with(visual, |view, cx| {
            view.commands
                .read(cx)
                .state
                .items
                .iter()
                .find(|item| item.id == info.id)
                .unwrap()
                .services[0]
                .port
        });
        let selector = format!("session-service-{port}");
        assert!(ports.read_with(visual, |ports, _| ports.local_port(port).is_none()));
        tap(visual, &selector);
        wait(visual, |cx| ports.read(cx).local_port(port).is_some());
        let local_port = ports.read_with(visual, |ports, _| ports.local_port(port).unwrap());
        assert_eq!(
            visual.opened_url(),
            Some(format!("http://127.0.0.1:{local_port}/app?q=1"))
        );
        tap(visual, &selector);
        assert_eq!(
            ports.read_with(visual, |ports, _| ports.local_port(port)),
            Some(local_port)
        );
        assert!(visual.debug_bounds("composer-background").is_some());
        tap(visual, "composer-background");
        assert!(visual.debug_bounds("background-output").is_none());
        let command = Box::leak(format!("background-command-{}", info.id).into_boxed_str());
        let title = visual.debug_bounds(command).unwrap();
        assert!(
            title.size.height <= px(24.),
            "multiline commands stay on one line"
        );
        assert!(title.right() <= visual.debug_bounds("background-list").unwrap().right());
        tap(visual, command);
        wait(visual, |cx| {
            view.read(cx)
                .commands
                .read(cx)
                .state
                .output
                .as_ref()
                .is_some_and(|output| output.stdout.text.contains("service heartbeat"))
        });
        visual.executor().advance_clock(Duration::from_secs(1));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let output = visual.debug_bounds("background-output").unwrap();
        assert_eq!(output.size.height, px(5. * 20.));
        let content = visual.debug_bounds("background-output-content").unwrap();
        assert!(content.size.height > output.size.height);
        visual.simulate_mouse_move(output.center(), None, Modifiers::default());
        visual.simulate_event(ScrollWheelEvent {
            position: output.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
            ..Default::default()
        });
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(visual.debug_bounds("background-output").unwrap(), output);
        assert!(
            visual
                .debug_bounds("background-output-content")
                .unwrap()
                .top()
                < content.top()
        );
        tap(visual, command);
        wait(visual, |cx| {
            let panel = view.read(cx).commands.read(cx);
            panel.selected.borrow().is_none() && panel.state.output.is_none()
        });
        assert!(visual.debug_bounds("background-output").is_none());
        tap(visual, command);
        wait(visual, |cx| {
            view.read(cx).commands.read(cx).state.output.is_some()
        });
        let port: u16 =
            std::fs::read_to_string(fixture.directory.path().join("project/service.port"))
                .unwrap()
                .parse()
                .unwrap();
        let mut connection = TcpStream::connect(("127.0.0.1", local_port)).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        connection
            .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        connection.read_to_string(&mut response).unwrap();
        assert!(response.contains("background-http-ready"));
        drop(connection);
        // The API cannot address a different session's task by guessing its ID.
        let error = fixture
            .runtime
            .block_on(
                fixture
                    .binding
                    .client
                    .execute(fixture.binding.client.prepare(Command::ReadCommand {
                        session: SessionId::new(),
                        id: info.id,
                    })),
            )
            .unwrap_err();
        assert_eq!(error.code, sailry_protocol::ErrorCode::NotFound);
        // Unmounting during navigation is not the explicit close-session action.
        visual.update(|window, _| window.remove_window());
        drop(view);
        assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session);
        visual.update(|_, cx| view.update(cx, |view, cx| view.bind_ports(ports.clone(), cx)));
        assert_eq!(
            ports.read_with(visual, |ports, _| ports.local_port(port)),
            Some(local_port)
        );
        wait(visual, |cx| {
            view.read(cx).commands.read(cx).state.items.len() == 2
                && view.read(cx).contributions.read(cx).ready(cx)
        });
        tap(visual, "composer-background");
        tap(visual, command);
        wait(visual, |cx| {
            view.read(cx).commands.read(cx).state.output.is_some()
        });
        // Settle the popup's measured anchor before comparing hover geometry.
        fixture::hover(visual, "background-list");
        let title = visual.debug_bounds(command).unwrap();
        visual.simulate_mouse_move(title.center(), None, Modifiers::default());
        visual.update(|window, cx| window.draw(cx).clear(cx));
        assert_eq!(visual.debug_bounds(command).unwrap(), title);
        let stop = visual
            .debug_bounds(Box::leak(
                format!("background-stop-{}", info.id).into_boxed_str(),
            ))
            .unwrap();
        let header = visual
            .debug_bounds(Box::leak(
                format!("background-header-{}", info.id).into_boxed_str(),
            ))
            .unwrap();
        assert_eq!(stop.right(), header.right());
        assert!(stop.top() >= header.top() && stop.bottom() <= header.bottom());
        assert!((stop.center().y - title.center().y).abs() <= px(1.));
        assert_eq!(
            title.right(),
            header.right(),
            "the stop action does not reserve title space"
        );
        let panel = view.read_with(visual, |view, _| view.commands.clone());
        panel.update_in(visual, |panel, window, cx| {
            panel.stop_command(RequestId::new(), window, cx)
        });
        wait(visual, |cx| {
            panel.read(cx).action.is_none()
                && panel.read(cx).error == Some("background_stop_failed")
        });
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("background_stop_failed")
        );
        panel.read_with(visual, |panel, _| {
            assert_eq!(*panel.selected.borrow(), Some(info.id));
            let output = panel.state.output.as_ref().unwrap();
            assert_eq!(output.info.id, info.id);
            assert!(output.stdout.text.contains("service heartbeat"));
            assert!(
                panel
                    .state
                    .items
                    .iter()
                    .any(|item| item.id == info.id && active(item))
            );
        });
        visual.update(|window, cx| window.clear_notifications(cx));
        tap(
            visual,
            Box::leak(format!("background-stop-{}", info.id).into_boxed_str()),
        );
        wait(visual, |cx| {
            matches!(
                view.read(cx)
                    .commands
                    .read(cx)
                    .state
                    .items
                    .iter()
                    .find(|item| item.id == info.id)
                    .unwrap()
                    .status,
                CommandStatus::Finished(Outcome::Cancelled)
            )
        });
        assert!(visual.debug_bounds(command).is_none());
        assert!(visual.debug_bounds("composer-background").is_some());
        assert!(view.read_with(visual, |view, cx| {
            let panel = view.commands.read(cx);
            panel.selected.borrow().is_none() && panel.state.output.is_none()
        }));
        let remaining = view.read_with(visual, |view, cx| {
            view.commands
                .read(cx)
                .state
                .items
                .iter()
                .find(|info| active(info))
                .unwrap()
                .id
        });
        tap(visual, &format!("background-command-{remaining}"));
        wait(visual, |cx| {
            *view.read(cx).commands.read(cx).selected.borrow() == Some(remaining)
        });
        wait(visual, |cx| ports.read(cx).local_port(port).is_none());
        assert!(TcpStream::connect(("127.0.0.1", local_port)).is_err());
        assert!(
            visual
                .debug_bounds(Box::leak(selector.into_boxed_str()))
                .is_none()
        );
        assert!(TcpStream::connect(("127.0.0.1", port)).is_err());
        visual.update(|window, cx| view.update(cx, |view, cx| view.stop_background(window, cx)));
        wait(visual, |cx| {
            view.read(cx).commands.read(cx).state.items.is_empty()
        });
        assert!(visual.debug_bounds("composer-background").is_none());

        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
#[cfg(unix)]
fn hides_completed_commands(cx: &mut TestAppContext) {
    fixture::init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(
            remote,
            vec![
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"background":true,"command":"printf 'finished output'; exit 7"}),
                ),
                (
                    crate::agent_fixture::plugin_tool("commands", "run_command"),
                    json!({"background":true,"command":"printf 'active output'; while [ ! -e finish.flag ]; do sleep 0.05; done"}),
                ),
            ],
        );
        let mut config = fixture.session.config.clone();
        config.permission = sailry_protocol::Permission::Full;
        let Output::Session(session) = fixture.execute(Command::SetSessionConfig {
            session: fixture.session.id,
            expected_revision: 1,
            config,
        }) else {
            panic!("session expected")
        };
        fixture.execute(Command::SubmitTurn {
            session: session.id,
            expected_revision: session.revision,
            message: "Run background commands".into(),
        });
        let (view, visual) = fixture::open(cx, fixture.binding.clone(), session);
        wait(visual, |cx| {
            let panel = view.read(cx).commands.read(cx);
            panel.state.items.len() == 2
                && panel
                    .state
                    .items
                    .iter()
                    .any(|info| info.status == CommandStatus::Finished(Outcome::Exited(7)))
                && panel.state.items.iter().any(active)
        });
        let (running, finished) = view.read_with(visual, |view, cx| {
            let panel = view.commands.read(cx);
            (
                panel
                    .state
                    .items
                    .iter()
                    .find(|info| active(info))
                    .unwrap()
                    .id,
                panel
                    .state
                    .items
                    .iter()
                    .find(|info| !active(info))
                    .unwrap()
                    .id,
            )
        });
        tap(visual, "composer-background");
        assert!(
            visual
                .debug_bounds(Box::leak(
                    format!("background-command-{finished}").into_boxed_str()
                ))
                .is_none()
        );
        tap(visual, &format!("background-command-{running}"));
        wait(visual, |cx| {
            view.read(cx)
                .commands
                .read(cx)
                .state
                .output
                .as_ref()
                .is_some_and(|output| {
                    output.info.id == running && output.stdout.text.contains("active output")
                })
        });
        std::fs::write(
            fixture.directory.path().join("project/finish.flag"),
            "finish",
        )
        .unwrap();
        wait(visual, |cx| {
            let panel = view.read(cx).commands.read(cx);
            panel.state.items.len() == 2
                && panel
                    .state
                    .items
                    .iter()
                    .all(|info| matches!(info.status, CommandStatus::Finished(_)))
                && panel.selected.borrow().is_none()
                && panel.state.output.is_none()
        });
        assert!(visual.debug_bounds("composer-background").is_none());
        assert!(visual.debug_bounds("background-list").is_none());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
