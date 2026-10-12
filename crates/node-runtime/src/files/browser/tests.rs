use super::*;

fn volume(name: &str, path: &str) -> Location {
    Location {
        kind: LocationKind::Volume,
        name: name.into(),
        path: path.into(),
    }
}

#[test]
fn hides_internal_mounts() {
    let mounts = [
        volume("Macintosh HD", "/"),
        volume("Macintosh HD", "/System/Volumes/Data"),
        volume("Preboot", "/System/Volumes/Preboot"),
        volume("VM", "/private/var/vm"),
        volume("Web", "/Volumes/Web"),
        volume("Archive", "/Volumes/Archive"),
    ];
    let mut locations = Vec::new();
    add_volumes(&mut locations, mounts, true);
    assert_eq!(
        locations
            .iter()
            .map(|location| location.path.as_str())
            .collect::<Vec<_>>(),
        ["/", "/Volumes/Web", "/Volumes/Archive"]
    );
    assert!(
        locations
            .iter()
            .all(|location| location.kind == LocationKind::Volume)
    );
}

#[test]
fn retains_the_shortest_mount() {
    for mounts in [
        [volume("Disk", "/Volumes/Disk"), volume("Disk", "/")],
        [volume("Disk", "/"), volume("Disk", "/Volumes/Disk")],
    ] {
        let mut locations = Vec::new();
        add_volumes(&mut locations, mounts, false);
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, "/");
    }
}

#[test]
fn preserves_distinct_and_unnamed_volumes() {
    let mut locations = vec![Location {
        kind: LocationKind::Home,
        name: String::new(),
        path: "/home/user".into(),
    }];
    add_volumes(
        &mut locations,
        [
            volume("Home", "/home/user"),
            volume("First", "/Volumes/First"),
            volume("Second", "/Volumes/Second"),
            volume("", "/mnt/one"),
            volume("", "/mnt/two"),
        ],
        true,
    );
    assert_eq!(locations.len(), 5);
    assert_eq!(locations[0].kind, LocationKind::Home);
    assert_eq!(locations[1].name, "First");
    assert_eq!(locations[2].name, "Second");
    assert_eq!(locations[3].path, "/mnt/one");
    assert_eq!(locations[4].path, "/mnt/two");
}

#[test]
fn leaves_other_platform_paths_visible() {
    let mut locations = Vec::new();
    add_volumes(
        &mut locations,
        [
            volume("Data", "/System/Volumes/Data"),
            volume("VM", "/private/var/vm"),
            volume("System", r"C:\"),
            volume("Data drive", r"D:\"),
            volume("Near prefix", "/System/VolumesBackup"),
        ],
        false,
    );
    assert_eq!(locations.len(), 5);
    let mut macos = Vec::new();
    add_volumes(
        &mut macos,
        [volume("Near prefix", "/System/VolumesBackup")],
        true,
    );
    assert_eq!(macos.len(), 1);
}
