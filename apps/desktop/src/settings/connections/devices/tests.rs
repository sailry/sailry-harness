use super::*;
use core::prelude::v1::test;

fn device(local: bool, online: bool, route: Option<Route>, rtt_ms: Option<u64>) -> Device {
    Device {
        id: NodeId([7; 32]),
        local,
        known: KnownDevice {
            name: None,
            execution: true,
            platform: None,
        },
        online,
        address: None,
        connection: ConnectionInfo { route, rtt_ms },
    }
}

#[test]
fn routes_and_latency() {
    for (case, device, route, latency) in [
        (
            "local",
            device(true, true, Some(Route::Local), None),
            None,
            None,
        ),
        (
            "peer",
            device(false, true, Some(Route::P2p), Some(9)),
            Some("connection_route_p2p"),
            Some("9 ms"),
        ),
        (
            "relay",
            device(false, true, Some(Route::Relay), Some(9)),
            Some("connection_route_relay"),
            Some("9 ms"),
        ),
        (
            "disconnected",
            device(false, false, Some(Route::Relay), Some(120)),
            None,
            None,
        ),
        ("unknown", device(false, true, None, None), None, None),
    ] {
        assert_eq!(device.route_key(), route, "{case}: route");
        assert_eq!(device.latency_text().as_deref(), latency, "{case}: latency");
        if case == "local" {
            assert_eq!(device.status_key(), "composer_host_local", "{case}: status");
        }
    }
}

#[test]
fn controller_status_follows_service_presence() {
    let mut phone = device(false, true, Some(Route::P2p), Some(13));
    phone.known.execution = false;
    phone.observe(&BTreeSet::new());
    assert_eq!(phone.status_key(), "connections_offline");
    assert_eq!(phone.route_key(), None);
    assert_eq!(phone.latency_text(), None);
    phone.observe(&BTreeSet::from([phone.id]));
    assert_eq!(phone.status_key(), "connections_online");
    assert_eq!(phone.route_key(), Some("connection_route_p2p"));
    assert_eq!(phone.latency_text().as_deref(), Some("13 ms"));
    phone.observe(&BTreeSet::from([NodeId([8; 32])]));
    assert_eq!(phone.status_key(), "connections_offline");
    assert_eq!(phone.latency_text(), None);
}

#[test]
fn host_status_keeps_its_own_availability() {
    for online in [false, true] {
        let mut host = device(false, online, Some(Route::P2p), Some(13));
        host.observe(&BTreeSet::from([host.id]));
        assert_eq!(host.online, online);
        host.observe(&BTreeSet::new());
        assert_eq!(host.online, online);
    }
}
