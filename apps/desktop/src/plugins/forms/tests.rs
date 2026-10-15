use super::*;
use core::prelude::v1::test;
use gpui_shell::HostArguments;

#[test]
fn bounds_and_releases_draft_handles() {
    let module = module();
    module.validate().unwrap();
    let arguments = HostArguments::new([HostValue::from("Draft")]);
    let mut handles = Vec::new();
    for _ in 0..MAX_FIELDS {
        handles.push(module.call("createText", &arguments).unwrap());
    }
    assert!(module.call("createText", &arguments).is_err());
    let removed = handles.remove(0);
    module
        .call("releaseText", &HostArguments::new([removed.clone()]))
        .unwrap();
    let next = module.call("createText", &arguments).unwrap();
    assert_ne!(next.as_str(), removed.as_str());
    assert!(
        module
            .call("readText", &HostArguments::new([removed]))
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );
    let other = super::module();
    assert!(
        other
            .call("readText", &HostArguments::new([next]))
            .unwrap_err()
            .to_string()
            .contains("unavailable")
    );
}
