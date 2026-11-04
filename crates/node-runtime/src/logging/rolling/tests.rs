use super::*;
use std::fs;

fn files(directory: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_name().unwrap() != LOCK)
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn keeps_writer_ownership_scoped() {
    for (case_name, run) in [
        (
            "leases_one_writer_per_stream",
            (|case_name| {
                let directory = tempfile::tempdir().unwrap();
                let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
                writer.write_all(b"first\n").unwrap();
                writer.flush().unwrap();
                let archive = directory.path().join("current.log.5");
                fs::write(&archive, "retain until acquisition").unwrap();
                let error = Writer::new(directory.path(), 8, 5).err().unwrap();
                assert_eq!(error.kind(), io::ErrorKind::WouldBlock, "{case_name}");
                assert!(archive.exists(), "{case_name}");
                assert_eq!(
                    fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
                    "first\n",
                    "{case_name}"
                );
                drop(writer);
                let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
                assert!(!archive.exists(), "{case_name}");
                writer.write_all(b"x\n").unwrap();
                writer.flush().unwrap();
                assert_eq!(
                    fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
                    "first\nx\n",
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "keeps_profiles_and_applications_independent",
            (|case_name| {
                let directory = tempfile::tempdir().unwrap();
                let paths = [
                    directory.path().join("first/logs/desktop"),
                    directory.path().join("first/logs/host"),
                    directory.path().join("second/logs/desktop"),
                ];
                let mut writers = paths
                    .iter()
                    .map(|path| Writer::new(path, 8, 5).unwrap())
                    .collect::<Vec<_>>();
                for writer in &mut writers {
                    writer.write_all(b"ready\n").unwrap();
                    writer.flush().unwrap();
                }
                for path in paths {
                    assert_eq!(
                        fs::read_to_string(path.join(CURRENT)).unwrap(),
                        "ready\n",
                        "{case_name}"
                    );
                    assert_eq!(files(&path).len(), 1, "{case_name}");
                }
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn retains_bounded_complete_records() {
    for (case_name, run) in [
        (
            "bounds_active_and_four_archives",
            (|case_name| {
                let directory = tempfile::tempdir().unwrap();
                let mut writer = Writer::new(directory.path(), 16, 5).unwrap();
                for index in 0..20 {
                    writeln!(writer, "line {index:02}").unwrap();
                }
                writer.flush().unwrap();
                let paths = files(directory.path());
                assert_eq!(paths.len(), 5, "{case_name}");
                assert!(
                    paths
                        .iter()
                        .all(|path| fs::metadata(path).unwrap().len() <= 16),
                    "{case_name}"
                );
                assert_eq!(
                    fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
                    "line 18\nline 19\n",
                    "{case_name}"
                );
                assert!(
                    directory.path().join("current.log.4").is_file(),
                    "{case_name}"
                );
                assert!(
                    !directory.path().join("current.log.5").exists(),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "rotates_before_complete_unicode_records",
            (|case_name| {
                let directory = tempfile::tempdir().unwrap();
                let mut writer = Writer::new(directory.path(), 15, 5).unwrap();
                writer.write_all("🙂甲\n".as_bytes()).unwrap();
                writer.write_all("乙🙂\n".as_bytes()).unwrap();
                writer.flush().unwrap();
                assert_eq!(
                    fs::read_to_string(directory.path().join("current.log.1")).unwrap(),
                    "🙂甲\n",
                    "{case_name}"
                );
                assert_eq!(
                    fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
                    "乙🙂\n",
                    "{case_name}"
                );
                assert!(
                    files(directory.path())
                        .iter()
                        .all(|path| fs::metadata(path).unwrap().len() <= 15),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "appends_after_restart_and_honors_boundary",
            (|case_name| {
                let directory = tempfile::tempdir().unwrap();
                let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
                writer.write_all(b"first\n").unwrap();
                writer.flush().unwrap();
                drop(writer);
                let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
                writer.write_all(b"x\n").unwrap();
                writer.flush().unwrap();
                assert_eq!(
                    fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
                    "first\nx\n",
                    "{case_name}"
                );
                assert_eq!(files(directory.path()).len(), 1, "{case_name}");
                writer.write_all(b"next\n").unwrap();
                writer.flush().unwrap();
                assert_eq!(
                    fs::read_to_string(directory.path().join("current.log.1")).unwrap(),
                    "first\nx\n",
                    "{case_name}"
                );
                assert_eq!(
                    fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
                    "next\n",
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn reports_write_and_rotation_failures() {
    for (case_name, run) in [
        (
            "rejects_oversized_raw_records_explicitly",
            (|case_name| {
                let directory = tempfile::tempdir().unwrap();
                let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
                let error = writer.write_all("🙂🙂🙂\n".as_bytes()).unwrap_err();
                assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{case_name}");
                assert_eq!(writer.errors.load(Ordering::Relaxed), 1, "{case_name}");
                writer.write_all(b"valid\n").unwrap();
                writer.flush().unwrap();
                assert_eq!(
                    fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
                    "valid\n",
                    "{case_name}"
                );
            }) as fn(&str),
        ),
        (
            "reports_rotation_io_failures",
            (|case_name| {
                let directory = tempfile::tempdir().unwrap();
                let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
                writer.write_all(b"first\n").unwrap();
                writer.flush().unwrap();
                fs::create_dir(directory.path().join("current.log.4")).unwrap();
                fs::write(directory.path().join("current.log.3"), "older\n").unwrap();
                assert!(writer.write_all(b"second\n").is_err(), "{case_name}");
                assert_eq!(writer.errors.load(Ordering::Relaxed), 1, "{case_name}");
                fs::remove_dir(directory.path().join("current.log.4")).unwrap();
                assert_eq!(
                    writer.write_all(b"third\n").unwrap_err().kind(),
                    io::ErrorKind::BrokenPipe,
                    "{case_name}"
                );
                assert!(
                    files(directory.path())
                        .iter()
                        .all(|path| fs::metadata(path).unwrap().len() <= 8),
                    "{case_name}"
                );
            }) as fn(&str),
        ),
    ] {
        run(case_name);
    }
}

#[test]
fn retires_only_owned_daily_files() {
    let directory = tempfile::tempdir().unwrap();
    for name in ["2026-10-23.log", "2026-10-24.log"] {
        fs::write(directory.path().join(name), "old diagnostic").unwrap();
    }
    for name in ["2026-10-23", "notes.log", "2026-02-30.log", CURRENT] {
        fs::write(directory.path().join(name), "preserve").unwrap();
    }
    fs::create_dir(directory.path().join("2026-10-22.log")).unwrap();
    retire(directory.path()).unwrap();
    assert!(!directory.path().join("2026-10-23.log").exists());
    assert!(!directory.path().join("2026-10-24.log").exists());
    for name in ["2026-10-23", "notes.log", "2026-02-30.log", CURRENT] {
        assert_eq!(
            fs::read_to_string(directory.path().join(name)).unwrap(),
            "preserve"
        );
    }
    assert!(directory.path().join("2026-10-22.log").is_dir());
}
