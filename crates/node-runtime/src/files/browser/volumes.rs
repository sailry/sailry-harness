//! File locations need mount paths, not device telemetry or Finder metadata.
use super::*;

#[cfg(not(target_os = "macos"))]
pub(super) fn list() -> Result<Vec<Location>, Fault> {
    let disks =
        sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing());
    Ok(disks
        .list()
        .iter()
        .filter_map(|disk| {
            text(disk.mount_point()).ok().map(|path| Location {
                kind: LocationKind::Volume,
                name: disk.name().to_string_lossy().into_owned(),
                path,
            })
        })
        .collect())
}

#[cfg(target_os = "macos")]
pub(super) fn list() -> Result<Vec<Location>, Fault> {
    // sysinfo 0.39 queries CFURL volume properties even with DiskRefreshKind::nothing.
    // MNT_NOWAIT reads the kernel's retained mount table without filesystem refreshes:
    // https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/getfsstat.2.html
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if count < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let capacity = count as usize;
    let size = capacity
        .checked_mul(std::mem::size_of::<libc::statfs>())
        .and_then(|size| i32::try_from(size).ok())
        .ok_or_else(|| {
            Fault::new(
                ErrorCode::Unavailable,
                "mounted filesystem list exceeds the platform limit",
            )
        })?;
    let mut mounts = Vec::<libc::statfs>::with_capacity(capacity);
    // SAFETY: The allocation holds capacity statfs records and size is its checked byte size.
    let read = unsafe { libc::getfsstat(mounts.as_mut_ptr(), size, libc::MNT_NOWAIT) };
    if read < 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    // SAFETY: getfsstat initializes complete records up to the supplied buffer size.
    // A concurrently changed mount list is just this call's bounded snapshot.
    unsafe { mounts.set_len((read as usize).min(capacity)) };
    Ok(mounts
        .iter()
        .filter_map(|mount| {
            let path = mount
                .f_mntonname
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| *byte as u8)
                .collect::<Vec<_>>();
            location(&path, mount.f_flags)
        })
        .collect())
}

#[cfg(target_os = "macos")]
fn location(path: &[u8], flags: u32) -> Option<Location> {
    // Apple recommends these mount flags for local volumes shown by UI elements.
    // https://developer.apple.com/documentation/coreservices/kfseventstreameventflagmount
    if flags & libc::MNT_LOCAL as u32 == 0 || flags & libc::MNT_DONTBROWSE as u32 != 0 {
        return None;
    }
    let path = std::str::from_utf8(path).ok()?;
    let mounted = Path::new(path);
    if !mounted.is_absolute() {
        return None;
    }
    Some(Location {
        kind: LocationKind::Volume,
        name: mounted
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path)
            .to_owned(),
        path: path.to_owned(),
    })
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn uses_only_visible_local_mounts() {
        let local = libc::MNT_LOCAL as u32;
        assert!(location(b"/Volumes/Archive", 0).is_none());
        assert!(location(b"/Volumes/Archive", local | libc::MNT_DONTBROWSE as u32).is_none());
        let volume = location(b"/Volumes/Archive", local).unwrap();
        assert_eq!(
            volume,
            Location {
                kind: LocationKind::Volume,
                name: "Archive".into(),
                path: "/Volumes/Archive".into(),
            }
        );
        assert_eq!(location(b"/", local).unwrap().name, "/");
    }

    #[test]
    fn preserves_utf8_mount_paths() {
        let local = libc::MNT_LOCAL as u32;
        let path = "/Volumes/资料 🙂";
        let volume = location(path.as_bytes(), local).unwrap();
        assert_eq!(volume.path, path);
        assert_eq!(volume.name, "资料 🙂");
        assert!(location(b"/Volumes/\xff", local).is_none());
        assert!(location(b"relative", local).is_none());
    }

    #[test]
    fn reuses_shared_filters_and_root_deduplication() {
        let local = libc::MNT_LOCAL as u32;
        let mut locations = vec![Location {
            kind: LocationKind::Home,
            name: String::new(),
            path: "/Users/fixture".into(),
        }];
        let mounts = [
            b"/".as_slice(),
            b"/System/Volumes/Data",
            b"/private/var/vm",
            b"/Volumes/Archive",
        ];
        add_volumes(
            &mut locations,
            mounts.into_iter().filter_map(|path| location(path, local)),
            true,
        );
        add_volumes(&mut locations, location(b"/", local), true);
        assert_eq!(
            locations
                .iter()
                .map(|location| location.path.as_str())
                .collect::<Vec<_>>(),
            ["/Users/fixture", "/", "/Volumes/Archive"]
        );
        assert_eq!(locations[1].kind, LocationKind::Volume);
    }
}
