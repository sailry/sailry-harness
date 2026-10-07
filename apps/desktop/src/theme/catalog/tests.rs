use super::*;
use core::prelude::v1::test;

fn catalog(loaded: &Loaded) -> Catalog {
    let mut builtin = Package::preview(loaded);
    builtin.id = "builtin".into();
    builtin.manifest = None;
    builtin.images.clear();
    Catalog {
        packages: vec![builtin, Package::preview(loaded)],
        selected: 1,
        root: None,
        error: None,
    }
}

#[test]
fn replaces_in_place_at_package_limit() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let mut loaded = Loaded::read(&source, true).unwrap();
    let mut catalog = catalog(&loaded);
    for index in 0..31 {
        let mut package = Package::preview(&loaded);
        package.id = format!("other-{index}");
        catalog.packages.push(package);
    }
    loaded.manifest.name = "Updated Tide".into();
    catalog.add(loaded).unwrap();
    assert_eq!(catalog.packages.len(), 33);
    assert_eq!(catalog.selected, 1);
    assert_eq!(catalog.packages[1].name.as_ref(), "Updated Tide");
    assert_eq!(catalog.packages[2].id, "other-0");
    let mut loaded = Loaded::read(&source, true).unwrap();
    loaded.manifest.id = "new-theme".into();
    assert!(catalog.add(loaded).is_err());
    assert_eq!(catalog.packages.len(), 33);
}

#[test]
fn accounts_for_replaced_images() {
    let directory = tempfile::tempdir().unwrap();
    let source = crate::theme::fixture::write(directory.path(), "tide");
    let mut loaded = Loaded::read(&source, true).unwrap();
    let image = Arc::new(RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::new(4096, 2048),
    )]));
    loaded.images = BTreeMap::from([("assets/large.png".into(), image)]);
    let mut catalog = catalog(&loaded);
    for id in ["shore", "river", "lake"] {
        let mut package = Package::preview(&loaded);
        package.id = id.into();
        catalog.packages.push(package);
    }
    // The catalog is at 128 MiB. Replacing its 32 MiB package must not double-count it.
    catalog.check(&loaded).unwrap();
    loaded.manifest.id = "new-theme".into();
    assert!(catalog.check(&loaded).is_err());
    loaded.manifest.id = "tide".into();
    loaded.images.insert(
        "assets/extra.png".into(),
        loaded.images["assets/large.png"].clone(),
    );
    assert!(catalog.add(loaded).is_err());
    assert_eq!(catalog.packages.len(), 5);
    assert_eq!(catalog.packages[1].images.len(), 1);
    assert_eq!(catalog.selected, 1);
}
