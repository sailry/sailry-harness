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
fn leases_one_writer_per_stream() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
    writer.write_all(b"first\n").unwrap();
    writer.flush().unwrap();
    let archive = directory.path().join("current.log.5");
    fs::write(&archive, "retain until acquisition").unwrap();
    let error = Writer::new(directory.path(), 8, 5).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert!(archive.exists());
    assert_eq!(
        fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
        "first\n"
    );
    drop(writer);
    let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
    assert!(!archive.exists());
    writer.write_all(b"x\n").unwrap();
    writer.flush().unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
        "first\nx\n"
    );
}

#[test]
fn keeps_profiles_and_applications_independent() {
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
        assert_eq!(fs::read_to_string(path.join(CURRENT)).unwrap(), "ready\n");
        assert_eq!(files(&path).len(), 1);
    }
}

#[test]
fn bounds_active_and_four_archives() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = Writer::new(directory.path(), 16, 5).unwrap();
    for index in 0..20 {
        writeln!(writer, "line {index:02}").unwrap();
    }
    writer.flush().unwrap();
    let paths = files(directory.path());
    assert_eq!(paths.len(), 5);
    assert!(
        paths
            .iter()
            .all(|path| fs::metadata(path).unwrap().len() <= 16)
    );
    assert_eq!(
        fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
        "line 18\nline 19\n"
    );
    assert!(directory.path().join("current.log.4").is_file());
    assert!(!directory.path().join("current.log.5").exists());
}

#[test]
fn rotates_before_complete_unicode_records() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = Writer::new(directory.path(), 15, 5).unwrap();
    writer.write_all("🙂甲\n".as_bytes()).unwrap();
    writer.write_all("乙🙂\n".as_bytes()).unwrap();
    writer.flush().unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("current.log.1")).unwrap(),
        "🙂甲\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
        "乙🙂\n"
    );
    assert!(
        files(directory.path())
            .iter()
            .all(|path| fs::metadata(path).unwrap().len() <= 15)
    );
}

#[test]
fn appends_after_restart_and_honors_boundary() {
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
        "first\nx\n"
    );
    assert_eq!(files(directory.path()).len(), 1);
    writer.write_all(b"next\n").unwrap();
    writer.flush().unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join("current.log.1")).unwrap(),
        "first\nx\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
        "next\n"
    );
}

#[test]
fn rejects_oversized_raw_records_explicitly() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
    let error = writer.write_all("🙂🙂🙂\n".as_bytes()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(writer.errors.load(Ordering::Relaxed), 1);
    writer.write_all(b"valid\n").unwrap();
    writer.flush().unwrap();
    assert_eq!(
        fs::read_to_string(directory.path().join(CURRENT)).unwrap(),
        "valid\n"
    );
}

#[test]
fn reports_rotation_io_failures() {
    let directory = tempfile::tempdir().unwrap();
    let mut writer = Writer::new(directory.path(), 8, 5).unwrap();
    writer.write_all(b"first\n").unwrap();
    writer.flush().unwrap();
    fs::create_dir(directory.path().join("current.log.4")).unwrap();
    fs::write(directory.path().join("current.log.3"), "older\n").unwrap();
    assert!(writer.write_all(b"second\n").is_err());
    assert_eq!(writer.errors.load(Ordering::Relaxed), 1);
    fs::remove_dir(directory.path().join("current.log.4")).unwrap();
    assert_eq!(
        writer.write_all(b"third\n").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert!(
        files(directory.path())
            .iter()
            .all(|path| fs::metadata(path).unwrap().len() <= 8)
    );
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
