use super::*;

#[gpui::test]
fn confirms_quit(cx: &mut TestAppContext) {
    let (shell, mut visual) = setup(cx);
    visual.simulate_keystrokes("secondary-q");
    assert!(visual.has_pending_prompt());
    assert!(shell.read_with(&visual, |shell, _| shell.closing));
    visual.simulate_keystrokes("secondary-q");
    visual.simulate_prompt_answer(&tr("settings_cancel"));
    visual.run_until_parked();
    assert!(!visual.has_pending_prompt());
    assert!(!shell.read_with(&visual, |shell, _| shell.closing));
    visual.dispatch_action(Quit);
    assert!(visual.has_pending_prompt());
    visual.simulate_prompt_answer(&tr("app_exit"));
    visual.run_until_parked();
    assert!(!visual.has_pending_prompt());
}
