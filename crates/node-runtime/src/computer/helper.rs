//! Locate an execution Node's packaged helper without starting package code.
use sailry_protocol::{ErrorCode, Fault};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) const TOKEN: &str = "${SAILRY_COMPUTER_USE}";

/// Graphical-session inputs the Linux helper needs to reach AT-SPI and the display.
const SESSION: &[&str] = &[
    "DBUS_SESSION_BUS_ADDRESS",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_CURRENT_DESKTOP",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
];

pub(crate) fn executable() -> Result<PathBuf, Fault> {
    #[cfg(target_os = "macos")]
    {
        static VERSION: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
        let version = VERSION.get_or_init(|| {
            sysinfo::System::os_version()?
                .split('.')
                .next()?
                .parse()
                .ok()
        });
        if !version.is_some_and(|major| major >= 13) {
            return Err(unavailable("computer use requires macOS 13 or later"));
        }
    }
    let platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "linux") {
        Platform::Linux
    } else {
        return Err(unavailable("computer use is unavailable on this platform"));
    };
    let executable = std::env::current_exe()
        .map_err(|_| unavailable("execution Node location is unavailable"))?;
    let helpers = std::env::var_os("SAILRY_HELPERS_DIR").map(PathBuf::from);
    let path = locate(platform, &executable, helpers.as_deref())?;
    validate(&path)?;
    Ok(path)
}

/// Pass the Node's graphical session to the helper; declared values take precedence.
pub(crate) fn session(env: &mut BTreeMap<String, String>) {
    if cfg!(target_os = "linux") {
        inherit(env, |name| std::env::var(name).ok());
    }
}

fn inherit(env: &mut BTreeMap<String, String>, read: impl Fn(&str) -> Option<String>) {
    for name in SESSION {
        if let Some(value) = read(name) {
            env.entry((*name).to_owned()).or_insert(value);
        }
    }
}

/// macOS charges TCC grants to a process tree's responsible app. Disclaiming
/// makes the signed helper app, rather than Desktop or Host, own them.
///
/// Call this after the environment has been cleared and fully assigned.
#[cfg(target_os = "macos")]
pub(crate) fn disclaim(command: &mut tokio::process::Command) {
    // A failed preparation keeps the ordinary exec, so diagnostics and MCP
    // still observe the same identity.
    if let Some(image) = disclaimed::Image::prepare(command.as_std()) {
        // SAFETY: the closure only passes memory prepared before fork to posix_spawn.
        unsafe {
            command.pre_exec(move || {
                image.exec();
                Ok(())
            });
        }
    }
}

#[cfg(target_os = "macos")]
mod disclaimed {
    use std::{
        ffi::{CString, OsStr},
        os::unix::ffi::OsStrExt,
    };

    type Disclaim = unsafe extern "C" fn(*mut libc::posix_spawnattr_t, libc::c_int) -> libc::c_int;

    /// A process image whose spawn attributes replace the forked child in place.
    pub(super) struct Image {
        program: CString,
        _strings: Vec<CString>,
        argv: Vec<*mut libc::c_char>,
        envp: Vec<*mut libc::c_char>,
        attributes: libc::posix_spawnattr_t,
    }

    // SAFETY: every pointer refers to strings and attributes owned by this image,
    // which the forked child only reads.
    unsafe impl Send for Image {}
    // SAFETY: as above; no method mutates the image after preparation.
    unsafe impl Sync for Image {}

    impl Image {
        pub(super) fn prepare(command: &std::process::Command) -> Option<Self> {
            // SAFETY: dlsym only reads the symbol tables of already-loaded images.
            let symbol = unsafe {
                libc::dlsym(
                    libc::RTLD_DEFAULT,
                    c"responsibility_spawnattrs_setdisclaim".as_ptr(),
                )
            };
            if symbol.is_null() {
                return None;
            }
            // SAFETY: libSystem exports this function with this C signature.
            let disclaim = unsafe { std::mem::transmute::<*mut libc::c_void, Disclaim>(symbol) };
            let text = |value: &OsStr| CString::new(value.as_bytes()).ok();
            let program = text(command.get_program())?;
            let mut arguments = vec![program.clone()];
            for argument in command.get_args() {
                arguments.push(text(argument)?);
            }
            let mut environment = Vec::new();
            for (key, value) in command.get_envs() {
                let Some(value) = value else { continue };
                let mut pair = key.as_bytes().to_vec();
                pair.push(b'=');
                pair.extend_from_slice(value.as_bytes());
                environment.push(CString::new(pair).ok()?);
            }
            let pointers = |strings: &[CString]| {
                strings
                    .iter()
                    .map(|value| value.as_ptr().cast_mut())
                    .chain(std::iter::once(std::ptr::null_mut()))
                    .collect::<Vec<_>>()
            };
            let argv = pointers(&arguments);
            let envp = pointers(&environment);
            let mut attributes: libc::posix_spawnattr_t = std::ptr::null_mut();
            // SAFETY: attributes is a valid out-pointer for initialization.
            if unsafe { libc::posix_spawnattr_init(&mut attributes) } != 0 {
                return None;
            }
            let mut strings = arguments;
            strings.extend(environment);
            let mut image = Self {
                program,
                _strings: strings,
                argv,
                envp,
                attributes,
            };
            // SAFETY: the attributes were initialized above and are owned by image.
            let configured = unsafe {
                libc::posix_spawnattr_setflags(
                    &mut image.attributes,
                    libc::POSIX_SPAWN_SETEXEC as libc::c_short,
                ) == 0
                    && disclaim(&mut image.attributes, 1) == 0
            };
            configured.then_some(image)
        }

        pub(super) fn exec(&self) {
            // SAFETY: all pointers refer to memory owned by this image. With
            // POSIX_SPAWN_SETEXEC the call replaces this process image and
            // returns only on failure, after which the ordinary exec continues.
            unsafe {
                libc::posix_spawn(
                    std::ptr::null_mut(),
                    self.program.as_ptr(),
                    std::ptr::null(),
                    &self.attributes,
                    self.argv.as_ptr(),
                    self.envp.as_ptr(),
                );
            }
        }
    }

    impl Drop for Image {
        fn drop(&mut self) {
            // SAFETY: attributes were initialized in prepare and are destroyed once.
            unsafe {
                libc::posix_spawnattr_destroy(&mut self.attributes);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Platform {
    Mac,
    Windows,
    Linux,
}

fn locate(platform: Platform, executable: &Path, helpers: Option<&Path>) -> Result<PathBuf, Fault> {
    let parent = executable
        .parent()
        .ok_or_else(|| unavailable("execution Node location is unavailable"))?;
    let helpers = helpers.map(Path::to_owned).unwrap_or_else(|| {
        if matches!(platform, Platform::Mac)
            && parent.file_name().is_some_and(|name| name == "MacOS")
            && parent
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "Contents")
        {
            parent.parent().expect("Contents directory").join("Helpers")
        } else {
            parent.join("helpers")
        }
    });
    Ok(helpers.join(match platform {
        Platform::Mac => "Sailry Computer Use.app/Contents/MacOS/agent-desktop",
        Platform::Windows => "agent-desktop.exe",
        Platform::Linux => "agent-desktop",
    }))
}

fn validate(path: &Path) -> Result<(), Fault> {
    let metadata = path
        .metadata()
        .map_err(|_| unavailable("computer helper is missing"))?;
    if !metadata.is_file() {
        return Err(unavailable("computer helper is not a regular executable"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(unavailable("computer helper is not executable"));
        }
    }
    Ok(())
}

fn unavailable(message: &str) -> Fault {
    Fault::new(ErrorCode::Unavailable, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_packaged_layouts() {
        let suffix = "Sailry Computer Use.app/Contents/MacOS/agent-desktop";
        assert_eq!(
            locate(
                Platform::Mac,
                Path::new("/Applications/Sailry.app/Contents/MacOS/sailry"),
                None
            )
            .unwrap(),
            Path::new("/Applications/Sailry.app/Contents/Helpers").join(suffix)
        );
        assert_eq!(
            locate(Platform::Mac, Path::new("/host/sailry-host"), None).unwrap(),
            Path::new("/host/helpers").join(suffix)
        );
        assert_eq!(
            locate(Platform::Windows, Path::new("/host/sailry-host.exe"), None).unwrap(),
            Path::new("/host/helpers/agent-desktop.exe")
        );
        assert_eq!(
            locate(Platform::Linux, Path::new("/host/sailry-host"), None).unwrap(),
            Path::new("/host/helpers/agent-desktop")
        );
        for platform in [Platform::Mac, Platform::Windows, Platform::Linux] {
            let path = locate(
                platform,
                Path::new("/host/sailry"),
                Some(Path::new("/development/helpers")),
            )
            .unwrap();
            assert!(path.starts_with("/development/helpers"));
        }
    }

    #[test]
    fn rejects_missing_and_nonexecutables() {
        let root = tempfile::tempdir().unwrap();
        assert!(validate(&root.path().join("missing")).is_err());
        assert!(validate(root.path()).is_err());
        let file = root.path().join("helper");
        std::fs::write(&file, "fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(validate(&file).is_err());
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        validate(&file).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn disclaims_responsibility() {
        type Responsible = unsafe extern "C" fn(libc::pid_t) -> libc::pid_t;
        // SAFETY: dlsym only reads the symbol tables of already-loaded images.
        let symbol = unsafe {
            libc::dlsym(
                libc::RTLD_DEFAULT,
                c"responsibility_get_pid_responsible_for_pid".as_ptr(),
            )
        };
        assert!(!symbol.is_null(), "responsibility lookup is unavailable");
        // SAFETY: libSystem exports this function with this C signature.
        let responsible = unsafe { std::mem::transmute::<*mut libc::c_void, Responsible>(symbol) };
        for disclaimed in [false, true] {
            let mut command = tokio::process::Command::new("/bin/sleep");
            command.env_clear().arg("10").kill_on_drop(true);
            if disclaimed {
                disclaim(&mut command);
            }
            let mut child = command.spawn().unwrap();
            let pid = libc::pid_t::try_from(child.id().unwrap()).unwrap();
            // The forked child keeps its parent's responsibility until it replaces its image.
            let owner = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    // SAFETY: the lookup only reads the identity of this fixture's live child.
                    let owner = unsafe { responsible(pid) };
                    if !disclaimed || owner == pid {
                        break owner;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap_or(-1);
            assert_eq!(owner == pid, disclaimed);
            child.kill().await.unwrap();
        }
    }

    #[test]
    fn inherits_graphical_session() {
        let mut env = BTreeMap::from([
            ("AGENT_DESKTOP_HOME".to_owned(), "/data".to_owned()),
            ("DISPLAY".to_owned(), ":7".to_owned()),
        ]);
        inherit(&mut env, |name| match name {
            "DISPLAY" => Some(":0".into()),
            "WAYLAND_DISPLAY" => Some("wayland-0".into()),
            "DBUS_SESSION_BUS_ADDRESS" => Some("unix:path=/run/user/1000/bus".into()),
            "XDG_RUNTIME_DIR" => Some("/run/user/1000".into()),
            "SAILRY_PRIVATE" => Some("ignored".into()),
            _ => None,
        });
        assert_eq!(
            env,
            BTreeMap::from([
                ("AGENT_DESKTOP_HOME".to_owned(), "/data".to_owned()),
                (
                    "DBUS_SESSION_BUS_ADDRESS".to_owned(),
                    "unix:path=/run/user/1000/bus".to_owned()
                ),
                ("DISPLAY".to_owned(), ":7".to_owned()),
                ("WAYLAND_DISPLAY".to_owned(), "wayland-0".to_owned()),
                ("XDG_RUNTIME_DIR".to_owned(), "/run/user/1000".to_owned()),
            ])
        );
    }
}
