//! Scoped callbacks let domain services report real work without owning task state.
use sailry_protocol::installation::{Phase, Progress};
use std::sync::Arc;

pub(crate) type Reporter = Arc<dyn Fn(Phase, Option<Progress>) + Send + Sync>;

pub(crate) fn assets(reporter: Option<&Reporter>, progress: sailry_model_assets::Progress) {
    let Some(reporter) = reporter else { return };
    let phase = match progress.phase {
        sailry_model_assets::Phase::Inspecting => Phase::Inspecting,
        sailry_model_assets::Phase::Downloading => Phase::Downloading,
        sailry_model_assets::Phase::Validating => Phase::Validating,
        sailry_model_assets::Phase::Installing => Phase::Installing,
    };
    reporter(
        phase,
        (phase == Phase::Downloading).then_some(Progress::Percent {
            percent: progress.percent as u8,
            bytes_per_second: (phase == Phase::Downloading)
                .then_some(progress.bytes_per_second as u64),
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn measures_only_actual_downloads() {
        let values = Arc::new(Mutex::new(Vec::new()));
        let reported = values.clone();
        let reporter: Reporter =
            Arc::new(move |phase, progress| reported.lock().unwrap().push((phase, progress)));
        for phase in [
            sailry_model_assets::Phase::Inspecting,
            sailry_model_assets::Phase::Downloading,
            sailry_model_assets::Phase::Validating,
            sailry_model_assets::Phase::Installing,
        ] {
            assets(
                Some(&reporter),
                sailry_model_assets::Progress {
                    phase,
                    percent: 99.,
                    bytes_per_second: 100.,
                },
            );
        }
        let values = values.lock().unwrap();
        assert!(values[0].1.is_none());
        assert_eq!(
            values[1].1,
            Some(Progress::Percent {
                percent: 99,
                bytes_per_second: Some(100)
            })
        );
        assert!(values[2].1.is_none());
        assert!(values[3].1.is_none());
    }
}
