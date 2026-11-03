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
fn preserves_local_meaning() {
    let local = device(true, true, Some(Route::Local), None);
    assert_eq!(local.status_key(), "composer_host_local");
    assert_eq!(local.route_key(), None);
    assert_eq!(local.latency_text(), None);
}

#[test]
fn pairs_selected_route_with_rtt() {
    for (route, key) in [
        (Route::P2p, "connection_route_p2p"),
        (Route::Relay, "connection_route_relay"),
    ] {
        let remote = device(false, true, Some(route), Some(9));
        assert_eq!(remote.route_key(), Some(key));
        assert_eq!(remote.latency_text().as_deref(), Some("9 ms"));
    }
}

#[test]
fn does_not_infer_disconnected_routes() {
    for remote in [
        device(false, false, Some(Route::Relay), Some(120)),
        device(false, true, None, None),
    ] {
        assert_eq!(remote.route_key(), Some("connection_route_unknown"));
        assert_eq!(remote.latency_text().as_deref(), Some("—"));
    }
}

#[test]
fn controller_status_follows_service_presence() {
    let mut phone = device(false, true, Some(Route::P2p), Some(13));
    phone.known.execution = false;
    phone.observe(&BTreeSet::new());
    assert_eq!(phone.status_key(), "connections_offline");
    assert_eq!(phone.route_key(), Some("connection_route_unknown"));
    assert_eq!(phone.latency_text().as_deref(), Some("—"));
    phone.observe(&BTreeSet::from([phone.id]));
    assert_eq!(phone.status_key(), "connections_online");
    assert_eq!(phone.route_key(), Some("connection_route_p2p"));
    assert_eq!(phone.latency_text().as_deref(), Some("13 ms"));
    phone.observe(&BTreeSet::from([NodeId([8; 32])]));
    assert_eq!(phone.status_key(), "connections_offline");
    assert_eq!(phone.latency_text().as_deref(), Some("—"));
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
