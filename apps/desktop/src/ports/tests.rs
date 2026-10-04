use super::*;
use crate::preview::Page;
use core::prelude::v1::test;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
        assert!(Instant::now() < deadline, "port UI deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn draw(cx: &mut VisualTestContext) {
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}
#[track_caller]
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    // Kit dialog hit targets settle after its opening animation.
    std::thread::sleep(*gpui_kit::component::dialog::ANIMATION_DURATION);
    draw(cx);
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("missing {selector}"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    draw(cx);
}
#[gpui::test]
fn opens_closes_and_isolates_hosts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    rust_i18n::set_locale("zh-CN");
    let fixture = fixture::Fixture::new();
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::init(cx);
        crate::shell::init(cx);
        cx.set_global(fixture.services());
    });
    let mut entity = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let shell = cx.new(|cx| Shell::new(window, cx));
        shell.update(cx, |shell, cx| shell.navigate(Page::Host, window, cx));
        entity = Some(shell.clone());
        Root::new(shell, window, cx)
    });
    let shell = entity.unwrap();
    for index in 0..2 {
        let node = fixture.nodes[index].id();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                let live = shell.live.as_mut().unwrap();
                live.hosts
                    .insert(node, fixture.nodes[index].link().address());
                live.select(node, cx);
                shell.navigate(Page::Host, window, cx);
            })
        });
        wait(visual, |cx| {
            shell.read(cx).ports.contains_key(&node)
                && shell
                    .read(cx)
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.node == node)
        });
        let ports = shell.read_with(visual, |shell, _| shell.ports[&node].clone());
        assert!(ports.read_with(visual, |ports, _| ports.entries.is_empty()));
        visual.simulate_resize(size(px(1440.), px(900.)));
        visual.run_until_parked();
        visual.update(|window, cx| window.draw(cx).clear(cx));
        let viewport = visual.debug_bounds("live-overview").unwrap();
        let content = visual.debug_bounds("host-content").unwrap();
        assert!(content.size.width <= px(crate::preview::CONTENT_WIDTH));
        assert!((content.center().x - viewport.center().x).abs() <= px(1.));
        assert!(visual.debug_bounds("host-charts").is_some());
        assert!(visual.debug_bounds("port-start").is_none());
        assert!(visual.debug_bounds("live-add-project").is_none());
        if index == 1 {
            let project = shell.read_with(visual, |shell, _| {
                shell
                    .live
                    .as_ref()
                    .unwrap()
                    .view
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .projects[0]
                    .id
            });
            visual.update(|window, cx| {
                shell.update(cx, |shell, cx| {
                    shell.live_resource_action(
                        &crate::live::menus::Dispatch {
                            node,
                            target: crate::live::menus::Target::Project(project),
                            command: crate::live::menus::Command::Open,
                        },
                        window,
                        cx,
                    )
                });
            });
            wait(visual, |cx| shell.read(cx).page == Page::Project);
            assert!(visual.debug_bounds("host-ports").is_none());
        }
        let selected = fixture.nodes[0].id();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(selected, cx);
                shell.navigate(Page::Conversation, window, cx);
                shell.live_resource_action(
                    &crate::live::menus::Dispatch {
                        node,
                        target: crate::live::menus::Target::Host,
                        command: crate::live::menus::Command::Ports,
                    },
                    window,
                    cx,
                );
            })
        });
        draw(visual);
        assert!(visual.debug_bounds("port-picker").is_some());
        assert!(visual.debug_bounds("live-ports").is_none());
        click(visual, "port-add");
        shell.read_with(visual, |shell, _| {
            assert_eq!(shell.page, Page::Conversation);
            assert_eq!(shell.live.as_ref().unwrap().selected, selected);
        });
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(visual.debug_bounds("port-create").is_some());
        assert!(visual.debug_bounds("port-cancel").is_some());
        click(visual, "port-create");
        assert_eq!(
            ports.read_with(visual, |ports, _| ports.error),
            Some("port_invalid")
        );
        assert!(ports.read_with(visual, |ports, _| ports.entries.is_empty()));
        visual.update(|window, cx| {
            ports.read(cx).port.clone().update(cx, |input, cx| {
                input.set_value(fixture.server.port.to_string(), window, cx)
            })
        });
        click(visual, "port-create");
        wait(visual, |cx| !ports.read(cx).entries.is_empty());
        draw(visual);
        assert!(visual.debug_bounds("port-picker").is_some());
        click(visual, "port-add");
        click(visual, "port-create");
        assert_eq!(
            ports.read_with(visual, |ports, _| ports.error),
            Some("port_exists")
        );
        assert_eq!(ports.read_with(visual, |ports, _| ports.entries.len()), 1);
        click(visual, "port-cancel");
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        let local_port = ports.read_with(visual, |ports, _| ports.entries[0].forwarder.local_port);
        visual.update(|window, cx| {
            ports.update(cx, |ports, cx| {
                ports.open_service(
                    Source {
                        session: sailry_protocol::SessionId::new(),
                        command: sailry_protocol::RequestId::new(),
                        service: sailry_protocol::process::Service {
                            port: fixture.server.port,
                            url: format!("http://localhost:{}/app?q=1", fixture.server.port),
                        },
                    },
                    window,
                    cx,
                );
                assert_eq!(ports.entries.len(), 1);
            })
        });
        assert_eq!(
            visual.opened_url(),
            Some(format!("http://127.0.0.1:{local_port}/app?q=1"))
        );
        fixture.runtime.block_on(async {
            let mut socket =
                tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, local_port))
                    .await
                    .unwrap();
            socket.write_all(b"desktop port").await.unwrap();
            socket.shutdown().await.unwrap();
            let mut bytes = Vec::new();
            socket.read_to_end(&mut bytes).await.unwrap();
            assert_eq!(bytes, b"desktop port");
        });
        visual.simulate_keystrokes("escape");
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live.as_mut().unwrap().select(node, cx);
                shell.navigate(Page::Host, window, cx);
            })
        });
        wait(visual, |cx| shell.read(cx).page == Page::Host);
        assert!(visual.debug_bounds("port-start").is_none());
        assert!(visual.debug_bounds("host-port-0").is_some());
        let mut action = crate::live::menus::Dispatch {
            node: fixture.nodes[1 - index].id(),
            target: crate::live::menus::Target::Host,
            command: crate::live::menus::Command::OpenPort { port: local_port },
        };
        let before = visual.opened_url();
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.live_resource_action(&action, window, cx);
            })
        });
        assert_eq!(visual.opened_url(), before);
        action.node = node;
        visual.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                assert!(!shell.port_menu(node, cx).is_empty());
                shell.live_resource_action(&action, window, cx);
            })
        });
        assert_eq!(
            visual.opened_url(),
            Some(format!("http://127.0.0.1:{local_port}"))
        );
        click(visual, "host-ports");
        click(visual, "port-row-0");
        click(visual, "port-copy");
        assert_eq!(
            visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
            format!("http://127.0.0.1:{local_port}")
        );
        click(visual, "host-ports");
        click(visual, "port-row-0");
        click(visual, "port-back");
        click(visual, "port-row-0");
        click(visual, "port-close");
        wait(visual, |cx| ports.read(cx).entries.is_empty());
        visual.simulate_keystrokes("escape");
    }
    visual.update(|window, cx| {
        shell.update(cx, |shell, _| shell.ports.clear());
        window.remove_window();
    });
    fixture.close();
}
