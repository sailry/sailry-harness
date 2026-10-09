//! A native PTY grants access to its own device without broadening process policies.
use adk_sandbox::sandbox::WrappedCommand;
use sailry_protocol::{ErrorCode, Fault};
use std::{
    fmt::Write as _,
    os::unix::ffi::OsStrExt,
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
};

pub(super) fn protect(command: &mut WrappedCommand, paths: &[PathBuf]) -> Result<(), Fault> {
    let [flag, profile, _, ..] = command.args.as_mut_slice() else {
        return Err(unavailable("sandbox wrapper arguments are invalid"));
    };
    if flag.as_os_str() != std::ffi::OsStr::new("-p") {
        return Err(unavailable("sandbox wrapper arguments are invalid"));
    }
    let mut policy = profile
        .to_str()
        .ok_or_else(|| unavailable("sandbox profile is not valid UTF-8"))?
        .to_owned();
    // ReadOnly grants do not subtract ADK's parent ReadWrite grant. These
    // denials also cover metadata created after command admission.
    writeln!(
        policy,
        r#"(deny file-write* (regex #"(^|/)\.[gG][iI][tT](/|$)"))"#
    )
    .expect("writing to a String cannot fail");
    for path in paths {
        writeln!(
            policy,
            "(deny file-write* (subpath {}))",
            path_literal(path)
        )
        .expect("writing to a String cannot fail");
    }
    *profile = policy.into();
    Ok(())
}

pub(super) fn terminal(command: &mut WrappedCommand, device: Option<&Path>) -> Result<(), Fault> {
    let device = device
        .ok_or_else(|| unavailable("terminal device identity is unavailable for workspace access"))?
        .canonicalize()
        .map_err(super::runtime::failure)?;
    if device.parent() != Some(Path::new("/dev"))
        || !device
            .metadata()
            .map_err(super::runtime::failure)?
            .file_type()
            .is_char_device()
    {
        return Err(unavailable("native terminal device is invalid"));
    }
    let path = device
        .to_str()
        .ok_or_else(|| unavailable("native terminal device path is not valid UTF-8"))?;
    // The pinned MacOsEnforcer produces -p, the generated profile, then the program.
    let [flag, profile, _, ..] = command.args.as_mut_slice() else {
        return Err(unavailable(
            "terminal sandbox wrapper arguments are invalid",
        ));
    };
    if flag.as_os_str() != std::ffi::OsStr::new("-p") {
        return Err(unavailable(
            "terminal sandbox wrapper arguments are invalid",
        ));
    }
    let mut policy = profile
        .to_str()
        .ok_or_else(|| unavailable("terminal sandbox profile is not valid UTF-8"))?
        .to_owned();
    writeln!(
        policy,
        "(allow file-read-data file-write-data file-ioctl (literal {}))",
        literal(path)
    )
    .expect("writing to a String cannot fail");
    *profile = policy.into();
    Ok(())
}

fn literal(value: &str) -> String {
    let mut text = String::from("\"");
    for character in value.chars() {
        match character {
            '\\' => text.push_str("\\\\"),
            '"' => text.push_str("\\\""),
            character if character.is_ascii_control() => {
                write!(text, "\\x{:02x}", character as u32)
                    .expect("writing to a String cannot fail");
            }
            character => text.push(character),
        }
    }
    text.push('"');
    text
}

fn path_literal(path: &Path) -> String {
    if let Some(path) = path.to_str() {
        return literal(path);
    }
    let mut text = String::from("\"");
    for &byte in path.as_os_str().as_bytes() {
        match byte {
            b'\\' => text.push_str("\\\\"),
            b'"' => text.push_str("\\\""),
            0x20..=0x7e => text.push(char::from(byte)),
            _ => write!(text, "\\x{byte:02x}").expect("writing to a String cannot fail"),
        }
    }
    text.push('"');
    text
}

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adk_sandbox::sandbox::{SandboxEnforcer, SandboxPolicyBuilder, macos::MacOsEnforcer};

    fn command() -> WrappedCommand {
        MacOsEnforcer::new()
            .wrap_command(
                "/bin/zsh".as_ref(),
                &["-l".into()],
                &SandboxPolicyBuilder::new().build(),
            )
            .unwrap()
    }

    #[test]
    fn grants_only_owned_device() {
        let pair = portable_pty::native_pty_system()
            .openpty(portable_pty::PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let device = pair.master.tty_name().unwrap();
        let mut wrapped = command();
        let original = wrapped.args[1].to_str().unwrap().to_owned();
        terminal(&mut wrapped, Some(&device)).unwrap();
        let profile = wrapped.args[1].to_str().unwrap();
        assert_eq!(
            profile.strip_prefix(&original).unwrap(),
            format!(
                "(allow file-read-data file-write-data file-ioctl (literal {}))\n",
                literal(device.canonicalize().unwrap().to_str().unwrap())
            )
        );
        assert!(!profile.contains("(literal \"/dev/tty\")"));
        assert!(!profile.contains("(subpath \"/dev\")"));
        assert!(!profile.contains("(allow network*)"));
        assert_eq!(wrapped.args[0], "-p");
        assert_eq!(wrapped.args[2], "/bin/zsh");
        assert_eq!(wrapped.args[3], "-l");
    }

    #[test]
    fn rejects_missing_identity() {
        let mut wrapped = command();
        let original = wrapped.args.clone();
        assert_eq!(
            terminal(&mut wrapped, None).unwrap_err().code,
            ErrorCode::Unavailable
        );
        assert_eq!(wrapped.args, original);
        assert_eq!(
            terminal(&mut wrapped, Some(Path::new("/")))
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
        assert_eq!(wrapped.args, original);
    }

    #[test]
    fn escapes_policy_literals() {
        assert_eq!(
            literal("/dev/tty\"\\\n\t\u{7f}"),
            "\"/dev/tty\\\"\\\\\\x0a\\x09\\x7f\""
        );
    }

    #[test]
    fn appends_metadata_denials_after_grants() {
        let mut wrapped = command();
        let original = wrapped.args[1].to_str().unwrap().to_owned();
        protect(&mut wrapped, &[PathBuf::from("/work/metadata\"\\name")]).unwrap();
        let profile = wrapped.args[1].to_str().unwrap();
        assert_eq!(
            profile.strip_prefix(&original).unwrap(),
            concat!(
                "(deny file-write* (regex #\"(^|/)\\.[gG][iI][tT](/|$)\"))\n",
                "(deny file-write* (subpath \"/work/metadata\\\"\\\\name\"))\n"
            )
        );
        assert_eq!(wrapped.args[2], "/bin/zsh");
        assert_eq!(wrapped.args[3], "-l");
    }
}
