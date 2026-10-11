use super::*;
use crate::{
    backend::Services,
    shell::{Shell, ToggleSidebar},
};
use gpui_kit::component::ThemeMode;
use sailry_protocol::Command;
use serde_json::json;

pub(super) async fn run(
    scenario: Scenario,
    shell: Entity<Shell>,
    cx: &mut AsyncWindowContext,
) -> Result<(), String> {
    let services = cx
        .read_global::<Services, _>(|services, _, _| services.clone())
        .unwrap();
    let mut records = Vec::new();
    for (route, (node, session)) in ["local", "iroh"].into_iter().zip(scenario.sessions) {
        println!("Selecting {route} host");
        until(cx, "known host", |_, cx| {
            shell
                .read(cx)
                .live
                .as_ref()
                .unwrap()
                .hosts
                .contains_key(&node)
        })
        .await;
        cx.update(|window, cx| {
            cx.activate(true);
            shell.update(cx, |shell, cx| shell.select_live_host(node, window, cx))
        })
        .unwrap();
        let mut previous = String::new();
        until(cx, "connected host", |_, cx| {
            let state = shell.read(cx);
            let live = state.live.as_ref().unwrap();
            let diagnostic = format!(
                "expected={node:?}, selected={:?}, transport={:?}, connected={}, snapshot={:?}, session_present={}, error={:?}, page={:?}, pane={:?}",
                live.selected,
                live.transport.target(),
                live.view.connected,
                live.view.snapshot.as_ref().map(|snapshot| snapshot.node),
                live.view.snapshot.as_ref().is_some_and(|snapshot| snapshot.sessions.iter().any(|value| value.id == session.id)),
                live.view.error.as_ref().map(|error| error.code),
                state.page,
                state.splits.read(cx).active,
            );
            if diagnostic != previous {
                println!("Host connection state: {diagnostic}");
                previous = diagnostic;
            }
            live.selected == node
                && live.transport.target() == node
                && live.view.connected
                && live.view.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.node == node
                        && snapshot.sessions.iter().any(|value| value.id == session.id)
                })
        })
        .await;
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_session(session.clone(), window, cx)
            })
        })
        .unwrap();
        until(cx, "loaded conversation", |_, cx| {
            shell.read(cx).current_chat().is_some_and(|chat| {
                let chat = chat.read(cx);
                chat.binding().client.target() == node
                    && chat.session() == Some(session.id)
                    && chat.loaded_turns() >= 20
            })
        })
        .await;
        let client = cx
            .update(|_, cx| {
                let chat = shell.read(cx).current_chat().unwrap().read(cx);
                let client = chat.binding().client;
                assert_eq!(client.target(), node);
                assert_eq!(chat.session(), Some(session.id));
                client
            })
            .unwrap();
        for (index, (name, mode, width)) in [
            ("light", ThemeMode::Light, 900.),
            ("dark", ThemeMode::Dark, 1280.),
        ]
        .into_iter()
        .enumerate()
        {
            cx.update(|window, cx| {
                window.activate_window();
                crate::theme::select(Some(mode), window, cx);
                window.resize(size(px(width), px(820.)));
            })
            .unwrap();
            pause(cx, Duration::from_millis(200)).await;
            let job = services.runtime.spawn({
                let client = client.clone();
                let session = session.clone();
                async move { fixture::turns(&client, &session, "Live ", 4).await }
            });
            records.push(measure(cx, &shell, job, route, &format!("conversation-{name}")).await?);
            until(cx, "completed conversation projection", |_, cx| {
                shell.read(cx).current_chat().is_some_and(|chat| {
                    let chat = chat.read(cx);
                    chat.binding().client.target() == node
                        && chat.session() == Some(session.id)
                        && chat.completed_turns() == Some(24 + (index as u64 + 1) * 4)
                })
            })
            .await;
            pause(cx, Duration::from_millis(100)).await;
        }
        let info = services
            .runtime
            .spawn({
                let client = client.clone();
                let session = session.clone();
                async move { fixture::terminal(&client, &session).await }
            })
            .await
            .map_err(|error| format!("{route} terminal setup failed: {error}"))?;
        cx.update(|window, cx| {
            shell.update(cx, |shell, cx| {
                shell.reveal_terminal(session.project, &info, window, cx)
            })
        })
        .unwrap();
        pause(cx, Duration::from_millis(300)).await;
        let job = services.runtime.spawn({
            let client = client.clone();
            let info = info.clone();
            async move { fixture::output(&client, &info).await }
        });
        records.push(measure(cx, &shell, job, route, "terminal").await?);
        pause(cx, Duration::from_millis(100)).await;
        services
            .runtime
            .spawn(async move {
                fixture::execute(
                    &client,
                    Command::CloseTerminal {
                        worktree: session.worktree,
                        terminal: info.id,
                    },
                )
                .await;
            })
            .await
            .map_err(|error| format!("{route} terminal cleanup failed: {error}"))?;
    }
    std::fs::write(
        scenario.output.join("workload.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .map_err(|error| format!("workload results could not be saved: {error}"))?;
    println!("Native desktop workloads complete");
    Ok(())
}

async fn measure(
    cx: &mut AsyncWindowContext,
    shell: &Entity<Shell>,
    job: tokio::task::JoinHandle<()>,
    route: &str,
    workload: &str,
) -> Result<serde_json::Value, String> {
    println!("Workload start: {route}/{workload}");
    eprintln!("workload-start: {route}/{workload}");
    let started = Instant::now();
    let mut delays = Vec::new();
    while !job.is_finished() {
        let timer = Instant::now();
        pause(cx, Duration::from_millis(50)).await;
        delays.push(timer.elapsed().as_secs_f64() * 1000. - 50.);
        if delays.len() % 10 == 0 {
            let previous = cx
                .update(|window, cx| {
                    let previous = shell.read(cx).layout.sidebar_open;
                    let focus = shell.read(cx).focus.clone();
                    window.focus(&focus, cx);
                    window.dispatch_action(Box::new(ToggleSidebar), cx);
                    previous
                })
                .unwrap();
            until(cx, "sidebar action", |_, cx| {
                shell.read(cx).layout.sidebar_open != previous
            })
            .await;
        }
        if started.elapsed() >= Duration::from_secs(60) {
            job.abort();
            return Err(format!("{route}/{workload} completion deadline"));
        }
    }
    job.await
        .map_err(|error| format!("{route}/{workload} worker failed: {error}"))?;
    delays.sort_by(f64::total_cmp);
    assert!(!delays.is_empty());
    let record = json!({"route":route,"workload":workload,"seconds":started.elapsed().as_secs_f64(),
        "foreground_samples":delays.len(), "timer_delay_p95_ms":delays[(delays.len()-1)*95/100], "timer_delay_max_ms":delays.last()});
    println!("Workload end: {record}");
    eprintln!("workload-end: {route}/{workload}");
    Ok(record)
}
