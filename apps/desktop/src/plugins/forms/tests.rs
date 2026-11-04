use super::*;
use core::prelude::v1::test;
use gpui_shell::HostArguments;

#[test]
fn ranges_and_handles() {
    for (scenario, check) in [
        (
            "bounds_and_releases_draft_handles",
            (|_scenario: &str| {
                let module = module();
                module.validate().unwrap();
                let arguments = HostArguments::new([HostValue::from("Draft")]);
                let mut handles = Vec::new();
                for _ in 0..MAX_FIELDS {
                    handles.push(module.call("createText", &arguments).unwrap());
                }
                assert!(
                    module.call("createText", &arguments).is_err(),
                    "bounds and releases draft handles"
                );
                let removed = handles.remove(0);
                module
                    .call("releaseText", &HostArguments::new([removed.clone()]))
                    .unwrap();
                let next = module.call("createText", &arguments).unwrap();
                assert_ne!(
                    next.as_str(),
                    removed.as_str(),
                    "bounds and releases draft handles"
                );
                assert!(
                    module
                        .call("readText", &HostArguments::new([removed]))
                        .unwrap_err()
                        .to_string()
                        .contains("unavailable"),
                    "bounds and releases draft handles"
                );
                let other = super::module();
                assert!(
                    other
                        .call("readText", &HostArguments::new([next]))
                        .unwrap_err()
                        .to_string()
                        .contains("unavailable"),
                    "bounds and releases draft handles"
                );
            }) as fn(&str),
        ),
        (
            "validates_slider_ranges_and_handles",
            (|_scenario: &str| {
                let module = module();
                module.validate().unwrap();
                let arguments = |min: f64, max: f64, step: f64| {
                    HostArguments::new([
                        HostValue::Number(8.),
                        gpui_shell::HostObject::new()
                            .field("min", min)
                            .field("max", max)
                            .field("step", step)
                            .into(),
                    ])
                };
                for (min, max, step) in [(1., 1., 1.), (50., 1., 1.), (1., 50., 0.)] {
                    assert!(
                        module
                            .call("createSlider", &arguments(min, max, step))
                            .is_err(),
                        "validates slider ranges and handles"
                    );
                }
                let handle = module
                    .call("createSlider", &arguments(1., 50., 1.))
                    .unwrap();
                module
                    .call(
                        "setSlider",
                        &HostArguments::new([handle.clone(), HostValue::Number(12.)]),
                    )
                    .unwrap();
                module
                    .call("releaseSlider", &HostArguments::new([handle.clone()]))
                    .unwrap();
                assert!(
                    module
                        .call(
                            "setSlider",
                            &HostArguments::new([handle, HostValue::Number(8.)])
                        )
                        .unwrap_err()
                        .to_string()
                        .contains("unavailable"),
                    "validates slider ranges and handles"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
