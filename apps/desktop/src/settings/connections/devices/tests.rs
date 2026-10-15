use super::*;
use core::prelude::v1::test;

mod latency {
    use super::*;

    fn device(execution: bool, local: bool, latency: Option<Duration>) -> Device {
        Device {
            id: NodeId([7; 32]),
            local,
            known: KnownDevice {
                name: None,
                execution,
                platform: None,
            },
            latency,
        }
    }

    #[test]
    fn omits_missing_host_values() {
        for (local, latency) in [(true, Some(Duration::ZERO)), (false, None)] {
            assert_eq!(device(true, local, latency).latency_text(), None);
        }
    }

    #[test]
    fn preserves_measurements_and_controller_values() {
        for execution in [false, true] {
            for millis in [9, 1248] {
                assert_eq!(
                    device(execution, false, Some(Duration::from_millis(millis))).latency_text(),
                    Some(format!("{millis} ms")),
                );
            }
        }
        assert_eq!(
            device(false, false, None).latency_text().as_deref(),
            Some("—")
        );
    }
}
