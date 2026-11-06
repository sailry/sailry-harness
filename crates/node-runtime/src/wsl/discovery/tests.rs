use super::*;

fn utf16(text: &str, bom: bool, big_endian: bool) -> Vec<u8> {
    let mut bytes = if bom {
        if big_endian {
            vec![0xfe, 0xff]
        } else {
            vec![0xff, 0xfe]
        }
    } else {
        Vec::new()
    };
    bytes.extend(text.encode_utf16().flat_map(|unit| {
        if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        }
    }));
    bytes
}

#[test]
fn decodes_names_without_losing_unicode_or_spaces() {
    for bytes in [
        utf16("Ubuntu\r\n资料 Linux\r\nUbuntu\r\n\0", false, false),
        utf16("Ubuntu\r\n资料 Linux\r\nUbuntu\r\n", true, false),
        utf16("Ubuntu\r\n资料 Linux\r\nUbuntu\r\n", true, true),
        "Ubuntu\n资料 Linux\nUbuntu\n".as_bytes().to_vec(),
    ] {
        assert_eq!(distributions(&bytes).unwrap(), ["Ubuntu", "资料 Linux"]);
    }
    assert!(distributions(&[]).unwrap().is_empty());
    for bytes in [
        vec![0xff, 0xfe, 0x41],
        utf16("Ubuntu\0Other\r\n", false, false),
        vec![0x00, 0xd8],
        vec![0xff],
    ] {
        assert_eq!(
            distributions(&bytes).unwrap_err().code,
            ErrorCode::Unavailable
        );
    }
}

fn fixture(path: &std::path::Path, mode: &str) -> tokio::process::Command {
    #[cfg(unix)]
    let mut command = {
        let mut command = tokio::process::Command::new("/bin/sh");
        command.args([
            "-c",
            "if read -r line; then exit 77; fi; case \"$2\" in wait) sleep 30;; failed) cat \"$1\"; exit 1;; *) cat \"$1\";; esac",
            "wsl-fixture",
        ]).arg(path).arg(mode);
        command
    };
    #[cfg(windows)]
    let mut command = {
        let mut command = tokio::process::Command::new("powershell.exe");
        command.args([
            "-NoProfile", "-NonInteractive", "-Command",
            "if ([Console]::In.ReadToEnd().Length -ne 0) { exit 77 }; if ($env:SAILRY_WSL_MODE -eq 'wait') { Start-Sleep -Seconds 30 }; $bytes = [IO.File]::ReadAllBytes($env:SAILRY_WSL_FIXTURE); [Console]::OpenStandardOutput().Write($bytes, 0, $bytes.Length); if ($env:SAILRY_WSL_MODE -eq 'failed') { exit 1 }",
        ]);
        command
    };
    command
        .env("SAILRY_WSL_FIXTURE", path)
        .env("SAILRY_WSL_MODE", mode);
    command
}

#[tokio::test]
async fn queries_only_successful_bounded_output() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("资料 output.bin");
    let bytes = utf16("Ubuntu\r\n", false, false);
    std::fs::write(&path, &bytes).unwrap();
    let stop = CancellationToken::new();
    assert_eq!(
        query(fixture(&path, "ready"), &stop, DEADLINE)
            .await
            .unwrap(),
        bytes
    );
    std::fs::write(&path, b"private-fixture-output").unwrap();
    let error = query(fixture(&path, "failed"), &stop, DEADLINE)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(
        error.message,
        "WSL discovery failed with exit code 1 (0x00000001)"
    );
    std::fs::write(&path, []).unwrap();
    assert!(
        query(fixture(&path, "ready"), &stop, DEADLINE)
            .await
            .unwrap()
            .is_empty()
    );
    std::fs::write(&path, vec![b'a'; LIMIT + 1]).unwrap();
    let error = query(fixture(&path, "ready"), &stop, DEADLINE)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(
        error.message,
        "WSL discovery output exceeds the supported size"
    );
}

#[cfg(windows)]
mod registration {
    use super::*;

    #[test]
    fn detects_guid_subkeys_without_hiding_corruption() {
        let transaction = windows_registry::Transaction::new().unwrap();
        let path = format!(r"Software\SailryShellFixture-{}", uuid::Uuid::new_v4());
        let open = |suffix: &str| {
            windows_registry::CURRENT_USER
                .options()
                .read()
                .transaction(&transaction)
                .open(format!(r"{path}\{suffix}"))
        };
        let create = |suffix: &str| {
            windows_registry::CURRENT_USER
                .options()
                .read()
                .write()
                .create()
                .transaction(&transaction)
                .open(format!(r"{path}\{suffix}"))
        };
        assert!(!registered(open("missing")).unwrap());
        drop(create("empty").unwrap());
        assert!(!registered(open("empty")).unwrap());
        drop(create(r"metadata\AppxInstallerCache").unwrap());
        drop(create(r"metadata\{not-a-guid}").unwrap());
        assert!(!registered(open("metadata")).unwrap());
        drop(create(r"corrupt\{16317611-9e46-4076-90d8-ce9e7c9b2141}").unwrap());
        assert!(registered(open("corrupt")).unwrap());
        let distribution = create(r"installed\{aa6936d0-a384-4493-aa70-12b97cf6c028}").unwrap();
        distribution
            .set_string("DistributionName", "Ubuntu 资料")
            .unwrap();
        assert!(registered(open("installed")).unwrap());
    }

    #[test]
    fn reads_versions_only_for_listed_distributions() {
        let transaction = windows_registry::Transaction::new().unwrap();
        let path = format!(r"Software\SailryWslFixture-{}", uuid::Uuid::new_v4());
        let root = windows_registry::CURRENT_USER
            .options()
            .read()
            .write()
            .create()
            .transaction(&transaction)
            .open(&path)
            .unwrap();
        for (id, name, version) in [
            ("{aa6936d0-a384-4493-aa70-12b97cf6c028}", "Ubuntu 资料", 2),
            ("{16317611-9e46-4076-90d8-ce9e7c9b2141}", "Legacy", 1),
        ] {
            let key = windows_registry::CURRENT_USER
                .options()
                .read()
                .write()
                .create()
                .transaction(&transaction)
                .open(format!(r"{path}\{id}"))
                .unwrap();
            key.set_string("DistributionName", name).unwrap();
            key.set_u32("Version", version).unwrap();
        }
        assert_eq!(
            metadata(&root, vec!["Ubuntu 资料".into()]).unwrap(),
            vec![sailry_protocol::wsl::Distribution {
                name: "Ubuntu 资料".into(),
                version: 2
            }]
        );
        assert_eq!(
            metadata(&root, vec!["Legacy".into()]).unwrap()[0].version,
            1
        );
        assert_eq!(
            metadata(&root, vec!["Removed".into()]).unwrap_err().code,
            ErrorCode::Unavailable
        );
        let key = windows_registry::CURRENT_USER
            .options()
            .read()
            .write()
            .transaction(&transaction)
            .open(format!(r"{path}\{{aa6936d0-a384-4493-aa70-12b97cf6c028}}"))
            .unwrap();
        key.set_string("Version", "not-a-number").unwrap();
        assert_eq!(
            metadata(&root, vec!["Ubuntu 资料".into()])
                .unwrap_err()
                .code,
            ErrorCode::Unavailable
        );
    }

    #[test]
    fn distinguishes_absence_from_read_errors() {
        use windows_sys::Win32::Foundation::{
            ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND,
        };
        for code in [ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND] {
            let error =
                windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(code));
            assert!(!registered(Err(error)).unwrap());
        }
        let error = windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(
            ERROR_ACCESS_DENIED,
        ));
        let fault = registered(Err(error)).unwrap_err();
        assert_eq!(fault.code, ErrorCode::Unavailable);
        assert_eq!(
            fault.message,
            "WSL discovery registration read failed (HRESULT 0x80070005)"
        );
    }
}

#[tokio::test]
async fn reports_timeouts_and_cancellation() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("output.bin");
    std::fs::write(&path, []).unwrap();
    let stop = CancellationToken::new();
    let error = query(fixture(&path, "wait"), &stop, Duration::from_millis(100))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert_eq!(error.message, "WSL discovery timed out");
    stop.cancel();
    assert_eq!(
        query(fixture(&path, "ready"), &stop, DEADLINE)
            .await
            .unwrap_err()
            .code,
        ErrorCode::Cancelled
    );
}
