use super::*;

#[test]
fn preserves_distribution_as_one_argument() {
    let name = "Ubuntu 资料 ' & $(unexpected)";
    let environment = Environment::filtered([]);
    let command = launch(Path::new("/fixture/wsl.exe"), name, &environment);
    assert_eq!(
        command.as_std().get_args().collect::<Vec<_>>(),
        [
            "--distribution",
            name,
            "--cd",
            "~",
            "--exec",
            "/bin/sh",
            "-s"
        ]
        .map(std::ffi::OsStr::new)
    );
}

#[cfg(unix)]
mod preflight {
    use super::*;
    use std::os::unix::fs::{PermissionsExt as _, symlink};

    struct Fixture {
        root: tempfile::TempDir,
        bin: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let bin = root.path().join("bin");
            std::fs::create_dir(&bin).unwrap();
            let probe = bin.join("probe");
            std::fs::write(
                &probe,
                r#"#!/bin/sh
case "${0##*/}" in
  uname) if test "$1" = -s; then echo Linux; else echo "${FIXTURE_ARCH:-x86_64}"; fi ;;
  getconf) echo "${FIXTURE_LIBC:-glibc 2.34}" ;;
  cat) test "$1" = /proc/1/comm; echo "${FIXTURE_INIT:-systemd}" ;;
  systemctl) case "$*" in *cat*) test "${FIXTURE_SERVICE:-no}" = yes ;; '--user show --property=Version --value') test "${FIXTURE_MANAGER:-yes}" = yes ;; show*) echo "${FIXTURE_STATE:-running}" ;; esac ;;
  id) if test "$1" = -u; then echo 1000; else echo fixture; fi ;;
  loginctl) exit 0 ;;
esac
"#,
            )
            .unwrap();
            std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o700)).unwrap();
            for name in [
                "uname",
                "getconf",
                "cat",
                "systemctl",
                "id",
                "loginctl",
                "curl",
                "bash",
                "tar",
                "sha256sum",
                "awk",
            ] {
                symlink(&probe, bin.join(name)).unwrap();
            }
            Self { root, bin }
        }

        fn run(&self, environment: &[(&str, &str)]) -> std::process::Output {
            std::process::Command::new("/bin/sh")
                .args(["-c", PREFLIGHT])
                .env_clear()
                .env("HOME", self.root.path())
                .env("PATH", &self.bin)
                .envs(environment.iter().copied())
                .output()
                .unwrap()
        }
    }

    #[test]
    fn requires_supported_runtime() {
        let fixture = Fixture::new();
        for architecture in ["x86_64", "aarch64", "arm64"] {
            let output = fixture.run(&[("FIXTURE_ARCH", architecture)]);
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        for (environment, diagnostic) in [
            (("FIXTURE_ARCH", "riscv64"), "x64 or arm64"),
            (("FIXTURE_LIBC", "glibc 2.33"), "GNU libc 2.34"),
            (("FIXTURE_LIBC", "musl 1.2"), "GNU libc 2.34"),
            (("FIXTURE_INIT", "init"), "Enable systemd"),
            (("FIXTURE_MANAGER", "no"), "systemd user manager"),
            (
                ("FIXTURE_STATE", "maintenance"),
                "systemd manager is not running",
            ),
            (("FIXTURE_SERVICE", "yes"), "Host service already exists"),
        ] {
            let output = fixture.run(&[environment]);
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
        }
    }

    #[test]
    fn refuses_existing_data_and_dangling_links() {
        let fixture = Fixture::new();
        let profile = fixture.root.path().join(".sailry-host");
        symlink("missing-profile", &profile).unwrap();
        let output = fixture.run(&[]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("already exist"));
        assert_eq!(
            std::fs::read_link(&profile).unwrap(),
            Path::new("missing-profile")
        );
        std::fs::remove_file(&profile).unwrap();
        std::fs::create_dir(&profile).unwrap();
        std::fs::write(profile.join("preserve"), b"original").unwrap();
        assert!(!fixture.run(&[]).status.success());
        assert_eq!(
            std::fs::read(profile.join("preserve")).unwrap(),
            b"original"
        );
    }
}
