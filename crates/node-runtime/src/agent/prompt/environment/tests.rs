use super::*;

#[test]
fn captures_node_facts() {
    let environment = Environment::capture();
    let before = chrono::Local::now().format("%Y-%m-%d (UTC%:z)").to_string();
    let instruction = capture(&environment, true, false);
    let after = chrono::Local::now().format("%Y-%m-%d (UTC%:z)").to_string();
    assert!(instruction.starts_with("Execution Node environment\n"));
    assert!(instruction.contains(&format!("OS: {}\n", std::env::consts::OS)));
    assert!(instruction.contains(&format!("Architecture: {}\n", std::env::consts::ARCH)));
    assert!(
        [before, after]
            .iter()
            .any(|date| instruction.contains(&format!("Date: {date}\n")))
    );
    assert!(instruction.contains("Working directory: . (captured worktree)\n"));
    assert!(instruction.contains(&format!("Command shell: {:?}", environment.shell())));
    #[cfg(unix)]
    assert_eq!(environment.shell(), std::ffi::OsStr::new("/bin/sh"));
}

#[test]
fn omits_unavailable_local_tools() {
    let environment = Environment::capture();
    for (project, bound) in [(false, false), (true, true), (false, true)] {
        let instruction = capture(&environment, project, bound);
        assert!(instruction.starts_with("Execution Node environment\n"));
        assert!(instruction.contains("Date: "));
        assert!(!instruction.contains("Working directory: "));
        assert!(!instruction.contains("Command shell: "));
    }
}
