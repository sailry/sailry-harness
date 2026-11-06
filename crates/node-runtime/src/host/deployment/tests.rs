use super::*;

#[test]
fn normalizes_embedded_resources() {
    for resource in [
        include_str!("release.sh"),
        include_str!("../../wsl/preflight.sh"),
        include_str!("../../ssh/install/service.sh"),
    ] {
        let expected = resource.replace("\r\n", "\n");
        assert!(!expected.contains('\r'));
        assert_eq!(normalize(&expected), expected);
        assert_eq!(normalize(&expected.replace('\n', "\r\n")), expected);
    }
}

#[test]
fn preserves_literal_carriage_returns() {
    assert_eq!(
        normalize("set -eu\r\nprintf '%s' 'one\rtwo'\r\n"),
        "set -eu\nprintf '%s' 'one\rtwo'\n"
    );
}

#[test]
fn preserves_arguments_before_resource() {
    let relay = "https://relay.example/path\r\nnext";
    let selection = Selection::Custom {
        relays: vec![relay.into()],
    };
    let source = script("0.1.0", false, Some(&selection)).unwrap();
    let prefix = format!("install_version='0.1.0'\ninstall_update=0\nset -- '--relay' '{relay}'\n");
    let resource = source.strip_prefix(&prefix).unwrap();
    assert!(!resource.contains('\r'));
    assert_eq!(resource, normalize(include_str!("release.sh")));
}
