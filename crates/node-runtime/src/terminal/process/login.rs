// Adapted from sailry-code 67ae9fa0, terminal-host/src/shell.rs.
// See third_party_licenses/sailry-code-terminal.md.
use super::super::failure;
use portable_pty::CommandBuilder;
use sailry_protocol::Fault;
use std::{env, ffi::OsString};

pub(super) fn command(configured: &str) -> Result<CommandBuilder, Fault> {
    let account = current_user_account()?;
    let mut command = CommandBuilder::new_default_prog();
    command.env_clear();
    for (key, value) in environment(&account, env::vars_os()) {
        command.env(key, value);
    }
    if !configured.is_empty() {
        command.env("SHELL", which::which(configured).map_err(failure)?);
    }
    Ok(command)
}

// A new login must not inherit a parent terminal's prompt/integration state.
fn environment(
    account: &Account,
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
) -> Vec<(OsString, OsString)> {
    let mut values: Vec<_> = inherited
        .into_iter()
        .filter(|(key, _)| {
            let key = key.to_string_lossy();
            matches!(
                key.as_ref(),
                "DBUS_SESSION_BUS_ADDRESS"
                    | "DISPLAY"
                    | "SSH_AUTH_SOCK"
                    | "TMPDIR"
                    | "WAYLAND_DISPLAY"
                    | "XAUTHORITY"
                    | "XDG_RUNTIME_DIR"
                    | "__CF_USER_TEXT_ENCODING"
                    | "LANG"
                    | "LANGUAGE"
            ) || key.starts_with("LC_")
        })
        .collect();
    for (key, value) in [
        ("HOME", &account.home),
        ("USER", &account.name),
        ("LOGNAME", &account.name),
        ("SHELL", &account.shell),
    ] {
        values.push((key.into(), value.into()));
    }
    #[cfg(not(target_os = "macos"))]
    values.push((
        "PATH".into(),
        format!(
            "{}/.local/bin:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin",
            account.home
        )
        .into(),
    ));
    values
}

#[cfg(unix)]
struct Account {
    name: String,
    home: String,
    shell: String,
}

#[cfg(unix)]
fn current_user_account() -> Result<Account, Fault> {
    use std::{ffi::CStr, mem::MaybeUninit, ptr};

    let mut buffer = vec![0_u8; 16 * 1024];
    loop {
        let mut record = MaybeUninit::<libc::passwd>::uninit();
        let mut result = ptr::null_mut();
        let status = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                record.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };

        if status == libc::ERANGE {
            buffer.resize(buffer.len() * 2, 0);
            continue;
        }
        if status != 0 {
            return Err(failure(format!(
                "failed to read the current user account: OS error {status}"
            )));
        }
        if result.is_null() {
            return Err(failure("current user account was not found".to_owned()));
        }

        let record = unsafe { record.assume_init() };
        let read_field = |value: *const libc::c_char, field: &str| {
            if value.is_null() {
                return Err(failure(format!("current user account has no {field}")));
            }
            unsafe { CStr::from_ptr(value) }
                .to_str()
                .map(str::to_owned)
                .map_err(|error| {
                    failure(format!(
                        "current user account {field} is not valid UTF-8: {error}"
                    ))
                })
        };

        return Ok(Account {
            name: read_field(record.pw_name, "name")?,
            home: read_field(record.pw_dir, "home directory")?,
            shell: read_field(record.pw_shell, "shell")?,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn isolates_parent_integration() {
        let account = Account {
            name: "fixture".into(),
            home: "/fixture/home".into(),
            shell: "/bin/zsh".into(),
        };
        let inherited = [
            ("HOME", "/other/home"),
            ("SHELL", "/bin/sh"),
            ("ZDOTDIR", "/parent/integration"),
            ("STARSHIP_SHELL", "zsh"),
            ("DMUX_TERMINAL_ID", "parent"),
            ("GHOSTTY_RESOURCES_DIR", "/parent/ghostty"),
            ("SSH_AUTH_SOCK", "/session/agent"),
            ("LANG", "en_US.UTF-8"),
        ];
        let values: std::collections::BTreeMap<_, _> = environment(
            &account,
            inherited.map(|(key, value)| (key.into(), value.into())),
        )
        .into_iter()
        .collect();
        for key in [
            "ZDOTDIR",
            "STARSHIP_SHELL",
            "DMUX_TERMINAL_ID",
            "GHOSTTY_RESOURCES_DIR",
        ] {
            assert!(!values.contains_key(std::ffi::OsStr::new(key)));
        }
        for (key, expected) in [
            ("HOME", "/fixture/home"),
            ("SHELL", "/bin/zsh"),
            ("SSH_AUTH_SOCK", "/session/agent"),
            ("LANG", "en_US.UTF-8"),
        ] {
            assert_eq!(values.get(std::ffi::OsStr::new(key)).unwrap(), expected);
        }
    }
}
