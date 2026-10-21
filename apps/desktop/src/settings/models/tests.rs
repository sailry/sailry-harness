use super::*;
use core::prelude::v1::test;

#[test]
fn maps_observed_transfer_progress() {
    let percent = Progress::Percent {
        percent: 40,
        bytes_per_second: Some(2048),
    };
    let ModelStatus::Downloading(Some(progress)) = status(&Status::Downloading, Some(percent))
    else {
        panic!("measured download expected")
    };
    assert_eq!(
        progress,
        sailry_model_assets::Progress {
            phase: sailry_model_assets::Phase::Downloading,
            percent: 40.,
            bytes_per_second: 2048.,
        }
    );
    let counts = Progress::Counts {
        completed: 1,
        total: 4,
        bytes_per_second: None,
    };
    let ModelStatus::Downloading(Some(progress)) = status(&Status::Downloading, Some(counts))
    else {
        panic!("measured download expected")
    };
    assert_eq!((progress.percent, progress.bytes_per_second), (25., 0.));
    assert!(matches!(
        status(&Status::Downloading, None),
        ModelStatus::Downloading(None)
    ));
    assert!(matches!(
        status(&Status::Installed, Some(percent)),
        ModelStatus::Installed
    ));
    assert!(matches!(
        status(&Status::Missing, None),
        ModelStatus::Missing
    ));
}
