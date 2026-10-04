use super::*;
use sailry_protocol::plugin::ui::Intent;

pub(super) fn check(
    visual: &mut VisualTestContext,
    shell: &Entity<Shell>,
    runtime: &tokio::runtime::Runtime,
    action: &Dispatch,
    root: &std::path::Path,
) {
    let client = shell.read_with(visual, |shell, _| shell.live.as_ref().unwrap().client());
    let Target::Project(project) = action.target else {
        panic!("project expected")
    };
    dispatch(visual, shell, action, Command::Branches);
    let repository = git2::Repository::open(root).unwrap();
    let branch = repository.head().unwrap().shorthand().unwrap().to_owned();
    overlay(visual, shell, project, "git", &branch);
    choose(visual, &branch);
    overlay(visual, shell, project, "git", &tr("git_copy_branch"));
    visual.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("unchanged".into())));
    set(visual, shell, runtime, &client, "git", false);
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| cx.read_from_clipboard().unwrap().text().unwrap()),
        "unchanged"
    );
    let registry = visual.update(|window, cx| {
        shell.update(cx, |shell, cx| {
            shell.project_plugins(project, window, cx).unwrap()
        })
    });
    settle(visual, shell, |_, cx| {
        registry.read(cx).intent(Intent::GitBranches, cx).is_none()
    });
    visual.update(|_, cx| {
        registry.update(cx, |registry, cx| {
            assert!(
                registry
                    .invoke_intent(Intent::GitBranches, serde_json::Value::Null, cx)
                    .is_err()
            )
        })
    });

    // Git and Worktrees remain independent packages; mounted controls lose authority on disable.
    open(visual, shell, action);
    set(visual, shell, runtime, &client, "worktrees", false);
    settle(visual, shell, |_, cx| {
        registry.read(cx).intent(Intent::Worktrees, cx).is_none()
    });
    visual.update(|_, cx| {
        registry.update(cx, |registry, cx| {
            assert!(
                registry
                    .invoke_intent(Intent::Worktrees, serde_json::Value::Null, cx)
                    .is_err()
            )
        })
    });
    assert!(visual.debug_bounds("entry-0").is_none());
    set(visual, shell, runtime, &client, "worktrees", true);
    set(visual, shell, runtime, &client, "git", true);
}
fn set(
    visual: &mut VisualTestContext,
    shell: &Entity<Shell>,
    runtime: &tokio::runtime::Runtime,
    client: &Client,
    name: &str,
    enabled: bool,
) {
    let Output::Snapshot(snapshot) = runtime
        .block_on(client.execute(client.prepare(Request::Snapshot)))
        .unwrap()
    else {
        panic!("snapshot expected")
    };
    let entry = snapshot
        .plugins
        .iter()
        .find(|entry| entry.name == name)
        .unwrap();
    runtime
        .block_on(client.execute(client.prepare(Request::SetPluginEnabled {
            name: entry.name.clone(),
            expected_revision: entry.revision,
            enabled,
        })))
        .unwrap();
    settle(visual, shell, |shell, _| {
        shell
            .live
            .as_ref()
            .unwrap()
            .view
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| {
                snapshot
                    .plugins
                    .iter()
                    .find(|entry| entry.name == name)
                    .is_some_and(|entry| entry.enabled == enabled)
            })
    });
}
