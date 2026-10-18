use super::*;
use core::prelude::v1::test;

#[test]
fn excludes_nonrunning_commands() {
    assert!(active(&CommandStatus::Running));
    for status in [
        CommandStatus::Stopping,
        CommandStatus::Finished(Outcome::Exited(0)),
        CommandStatus::Finished(Outcome::Exited(1)),
        CommandStatus::Finished(Outcome::Signal(15)),
        CommandStatus::Finished(Outcome::TimedOut),
        CommandStatus::Finished(Outcome::Cancelled),
        CommandStatus::Finished(Outcome::Unknown("Unconfirmed cleanup".into())),
        CommandStatus::Failed("Execution failed".into()),
    ] {
        assert!(!active(&status), "{status:?}");
    }
}
