use super::*;
use std::{
    io::Read,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Fixture {
    leader: Child,
    member: Option<Child>,
}

impl Fixture {
    fn new() -> Self {
        let executable = std::env::current_exe().unwrap();
        let child = |role: &str, group| {
            let mut command = Command::new(&executable);
            command
                .args([
                    "--exact",
                    "process::macos::tests::classifies_exited_members",
                    "--nocapture",
                ])
                .env("SAILRY_GROUP_FIXTURE", role)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(group);
            command.spawn().unwrap()
        };
        let leader = child("leader", 0);
        let mut fixture = Self {
            leader,
            member: None,
        };
        fixture.member = Some(child("member", fixture.leader.id() as libc::pid_t));
        fixture
    }

    fn group(&self) -> Group {
        Group {
            id: self.leader.id() as libc::pid_t,
            // SAFETY: this only reads the test process's identity.
            owner: unsafe { libc::geteuid() },
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for child in std::iter::once(&mut self.leader).chain(self.member.iter_mut()) {
            // These retained Child handles own only this fixture's exact processes.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn classifies_exited_members() {
    match std::env::var("SAILRY_GROUP_FIXTURE").as_deref() {
        Ok("leader") => {
            let _ = std::io::stdin().read(&mut [0]);
            std::process::exit(0);
        }
        Ok("member") => std::process::exit(0),
        _ => {}
    }
    let mut fixture = Fixture::new();
    let group = fixture.group();
    let member = fixture.member.as_ref().unwrap().id() as libc::pid_t;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if information(member).is_some_and(|info| info.pbi_status == libc::SZOMB) {
            break;
        }
        assert!(Instant::now() < deadline, "fixture member did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !group.settled(),
        "the live leader must prevent classification"
    );
    // SAFETY: signal zero only probes this fixture's saved group.
    assert_eq!(unsafe { libc::killpg(group.id, 0) }, 0);
    fixture.leader.stdin.take();
    assert!(fixture.leader.wait().unwrap().success());
    // SAFETY: this group contains only the exact owned, unreaped member above.
    assert_eq!(unsafe { libc::killpg(group.id, libc::SIGKILL) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    assert!(group.settled());
    assert!(fixture.member.as_mut().unwrap().wait().unwrap().success());
    assert!(information(member).is_none());
    assert!(group.settled(), "the fully reaped group must be empty");
}

#[test]
fn rejects_uncertain_members() {
    let group = Group { id: 42, owner: 501 };
    // SAFETY: proc_bsdinfo consists only of integer fields and fixed byte arrays.
    let mut info = unsafe { std::mem::zeroed::<libc::proc_bsdinfo>() };
    info.pbi_pid = 43;
    info.pbi_pgid = 42;
    info.pbi_uid = 501;
    info.pbi_ruid = 501;
    info.pbi_svuid = 501;
    info.pbi_status = libc::SZOMB;
    assert!(group.exited(43, &info));
    for status in [0, libc::SRUN, libc::SSLEEP, libc::SSTOP] {
        info.pbi_status = status;
        assert!(!group.exited(43, &info));
    }
    info.pbi_status = libc::SZOMB;
    info.pbi_pid = 44;
    assert!(!group.exited(43, &info));
    info.pbi_pid = 43;
    info.pbi_pgid = 44;
    assert!(!group.exited(43, &info));
    info.pbi_pgid = 42;
    for field in ["effective", "real", "saved"] {
        match field {
            "effective" => info.pbi_uid = 0,
            "real" => info.pbi_ruid = 0,
            "saved" => info.pbi_svuid = 0,
            _ => unreachable!(),
        }
        assert!(!group.exited(43, &info), "{field}");
        info.pbi_uid = 501;
        info.pbi_ruid = 501;
        info.pbi_svuid = 501;
    }
    assert!(information(0).is_none());
    assert!(!Group { id: 0, owner: 501 }.settled());
}

#[test]
fn rejects_incomplete_enumeration() {
    assert_eq!(count(0, 0), Some(0));
    assert_eq!(count(1, 0), Some(1));
    assert_eq!(count(-1, 0), None);
    assert_eq!(count(0, libc::EPERM), None);
    assert_eq!(count(1, libc::EPERM), None);
    assert_eq!(count(CAPACITY as libc::c_int, 0), None);
    assert_eq!(count(CAPACITY as libc::c_int + 1, 0), None);
}
