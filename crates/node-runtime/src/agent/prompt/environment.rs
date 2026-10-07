//! Facts describe the execution Node, not its controller or a bound resource.
use crate::process::Environment;

pub(in crate::agent) fn capture(environment: &Environment, project: bool, bound: bool) -> String {
    let date = chrono::Local::now().format("%Y-%m-%d (UTC%:z)");
    let mut instruction = format!(
        "Execution Node environment\nOS: {}\nArchitecture: {}\nDate: {date}",
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    if project && !bound {
        instruction.push_str(&format!(
            "\nWorking directory: . (captured worktree)\nCommand shell: {:?}",
            environment.shell()
        ));
    }
    instruction
}

#[cfg(test)]
mod tests;
