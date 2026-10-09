use super::*;
use std::net::SocketAddr;

#[test]
fn rejects_nonpublic_literals_and_names() {
    for value in [
        "http://localhost/",
        "http://localhost./",
        "http://a.localhost/",
        "http://host.local/",
        "http://127.0.0.1/",
        "http://0x7f000001/",
        "http://2130706433/",
        "http://10.0.0.1/",
        "http://100.64.0.1/",
        "http://169.254.169.254/",
        "http://192.168.1.1/",
        "http://224.0.0.1/",
        "http://[::1]/",
        "http://[::ffff:127.0.0.1]/",
        "http://[fc00::1]/",
        "http://[fe80::1]/",
        "http://[64:ff9b::a00:1]/",
        "http://[2002:0a00:0001::1]/",
    ] {
        assert_eq!(
            url(value).unwrap_err().code,
            ErrorCode::PermissionDenied,
            "{value}"
        );
    }
}

#[test]
fn rejects_credentials_and_other_schemes() {
    for value in [
        "file:///etc/passwd",
        "ftp://example.com/",
        "https://user@example.com/",
        "https://user:pass@example.com/",
    ] {
        assert_eq!(url(value).unwrap_err().code, ErrorCode::InvalidRequest);
    }
    assert_eq!(
        url("https://example.com/path#fragment").unwrap().as_str(),
        "https://example.com/path"
    );
}

#[test]
fn validates_every_resolved_and_local_address() {
    let public: SocketAddr = "8.8.8.8:443".parse().unwrap();
    let private: SocketAddr = "10.0.0.1:443".parse().unwrap();
    assert!(addresses(&[public], &[]).is_ok());
    assert!(addresses(&[public, private], &[]).is_err());
    assert!(addresses(&[public], &[public.ip()]).is_err());
    assert!(addresses(&["[::ffff:8.8.8.8]:443".parse().unwrap()], &[public.ip()]).is_err());
    assert!(addresses(&[], &[]).is_err());
    assert!(addresses(&["[2606:4700:4700::1111]:443".parse().unwrap()], &[]).is_ok());
    for ip in [
        "0.0.0.0",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "255.255.255.255",
        "2001:db8::1",
        "2001::1",
        "3fff::1",
        "ff02::1",
    ] {
        assert!(!super::public(ip.parse().unwrap()), "{ip}");
    }
}
