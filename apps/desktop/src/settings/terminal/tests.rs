use super::*;
use core::prelude::v1::test;
use gpui_kit::component::select::SelectEvent;
use sailry_client::Client;
use sailry_node_runtime::Node;
use sailry_protocol::{Command, Output, terminal::Settings};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn wait(cx: &mut VisualTestContext, ready: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            ready(cx)
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "terminal settings UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read(runtime: &tokio::runtime::Runtime, client: &Client) -> Settings {
    let Output::TerminalSettings(settings) = runtime
        .block_on(client.execute(client.prepare(Command::ReadTerminalSettings)))
        .unwrap()
    else {
        panic!("settings expected")
    };
    settings
}

#[gpui::test]
fn preserves_host_drafts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let directory = tempfile::tempdir().unwrap();
    let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
    let local = runtime
        .block_on(Node::start(directory.path().join("local")))
        .unwrap();
    let remote = runtime
        .block_on(Node::start(directory.path().join("remote")))
        .unwrap();
    let address = runtime
        .block_on(local.link().pair(remote.link().invite().unwrap().ticket()))
        .unwrap();
    let transports: [Arc<dyn sailry_link::Transport>; 2] =
        [local.local(), local.link().remote(address)];
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        cx.set_global(crate::preferences::Preferences::open(
            directory.path().join("preferences.json"),
        ));
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| Workspace::new(window, cx));
        view.update(cx, |view, _| view.section = super::super::Section::Terminal);
        entity = Some(view.clone());
        Root::new(view, window, cx)
    });
    let workspace = entity.unwrap();
    let handle = visual.update(|window, _| window.window_handle());
    visual.simulate_window_resize(handle, size(px(1000.), px(1100.)));
    for (index, transport) in transports.iter().enumerate() {
        let node = transport.target();
        visual.update(|_, cx| {
            workspace.update(cx, |view, cx| {
                view.bind_providers(
                    transport.clone(),
                    runtime.clone(),
                    format!("Host {index}").into(),
                    cx,
                )
            })
        });
        wait(visual, |cx| {
            workspace
                .read(cx)
                .terminal
                .hosts
                .get(&node)
                .is_some_and(|host| host.read(cx).saved.is_some())
        });
        visual.update(|window, cx| {
            workspace.read(cx).terminal.hosts[&node]
                .clone()
                .update(cx, |host, cx| {
                    host.environment
                        .push(("UI_TERMINAL_FIXTURE".into(), format!("host-{index}")));
                    host.shell.update(cx, |select, cx| {
                        select.set_selected_value(&"/bin/sh".to_owned(), window, cx);
                        cx.emit(SelectEvent::Confirm(select.selected_value().cloned()));
                    });
                });
            window.draw(cx).clear(cx);
        });
        wait(visual, |cx| {
            workspace.read(cx).terminal.hosts[&node]
                .read(cx)
                .saved
                .as_ref()
                .unwrap()
                .revision
                == 1
        });
        assert_eq!(
            read(&runtime, &Client::new(transport.clone())).environment["UI_TERMINAL_FIXTURE"],
            format!("host-{index}")
        );
        visual.update(|window, cx| {
            workspace.read(cx).terminal.hosts[&node]
                .read(cx)
                .shell
                .clone()
                .update(cx, |select, cx| {
                    select.set_selected_value(&String::new(), window, cx);
                    cx.emit(SelectEvent::Confirm(select.selected_value().cloned()));
                });
        });
        wait(visual, |cx| {
            workspace.read(cx).terminal.hosts[&node]
                .read(cx)
                .saved
                .as_ref()
                .unwrap()
                .revision
                == 2
        });
        assert!(
            read(&runtime, &Client::new(transport.clone()))
                .shell
                .is_empty()
        );
        visual.update(|window, cx| {
            workspace.read(cx).terminal.hosts[&node]
                .clone()
                .update(cx, |host, cx| {
                    host.shell.update(cx, |select, cx| {
                        let value = format!("/pending/host-{index}");
                        select.set_items(vec![host::Shell(value.clone())], window, cx);
                        select.set_selected_value(&value, window, cx);
                    })
                })
        });
    }
    let node = local.id();
    visual.update(|_, cx| {
        workspace.update(cx, |view, cx| {
            view.bind_providers(transports[0].clone(), runtime.clone(), "Local".into(), cx)
        })
    });
    wait(visual, |cx| {
        workspace
            .read(cx)
            .provider_link
            .as_ref()
            .unwrap()
            .binding
            .client
            .target()
            == node
    });
    visual.update(|_, cx| {
        assert_eq!(
            workspace.read(cx).terminal.hosts[&node]
                .read(cx)
                .shell
                .read(cx)
                .selected_value()
                .map(String::as_str),
            Some("/pending/host-0")
        )
    });
    let client = Client::new(transports[0].clone());
    let mut updated = read(&runtime, &client);
    updated
        .environment
        .insert("EXTERNAL_CHANGE".into(), "retained".into());
    runtime
        .block_on(client.execute(client.prepare(Command::SaveTerminalSettings(updated))))
        .unwrap();
    wait(visual, |cx| {
        workspace.read(cx).terminal.hosts[&node].read(cx).error
            == Some("terminal_settings_conflict")
    });
    visual.update(|window, cx| {
        workspace.update(cx, |view, cx| {
            view.terminal.font_size.update(cx, |slider, cx| {
                slider.set_value(20., window, cx);
                cx.emit(SliderEvent::Release(slider.value()));
            });
            view.terminal
                .font
                .update(cx, |input, cx| input.set_value("Menlo", window, cx));
        });
        window.draw(cx).clear(cx);
    });
    visual.update(|window, cx| {
        workspace
            .read(cx)
            .terminal
            .font
            .clone()
            .update(cx, |input, cx| input.focus(window, cx))
    });
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    let saved = crate::preferences::Preferences::open(directory.path().join("preferences.json"));
    assert_eq!(saved.data.terminal.font_size, 20);
    assert_eq!(saved.data.terminal.font_family, "Menlo");
    visual.update(|window, _| window.remove_window());
    drop(workspace);
    runtime.block_on(local.shutdown()).unwrap();
    runtime.block_on(remote.shutdown()).unwrap();
}
