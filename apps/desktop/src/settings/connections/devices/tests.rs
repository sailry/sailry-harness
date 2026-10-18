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
        latency: online.then_some(Duration::from_secs(3)),
        address: None,
        connection: ConnectionInfo { route, rtt_ms },
    }
}

#[test]
fn preserves_local_meaning() {
    let local = device(true, true, Some(Route::Local), None);
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
        // Inspect took three seconds, but the displayed value is the selected QUIC path's RTT.
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
