//! One placement decision serves both toolbar and settings without changing values.
use super::{Contribution, Form, Slot, ui};

#[derive(Clone, Copy)]
pub(super) struct Layout {
    pub compact: bool,
    pub narrow: bool,
    pub sidebar: bool,
}

impl Layout {
    pub(super) fn includes(self, entry: &Contribution, form: Form) -> bool {
        let settings = match entry.slot {
            Slot::Composer => self.compact && entry.kind != ui::Kind::Indicator,
            Slot::Context => {
                self.compact
                    && (self.narrow || self.sidebar || entry.overflow == ui::Overflow::Menu)
            }
            _ => return true,
        };
        match form {
            Form::Menu => settings,
            Form::Toolbar | Form::Icons => !settings,
            Form::Project | Form::Model => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use serde_json::json;

    #[test]
    fn requested_destinations() {
        for (scenario, check) in [
            (
                "every_contribution_has_one_destination",
                (|_scenario: &str| {
                    for (compact, narrow) in [(false, false), (true, false), (true, true)] {
                        for sidebar in [false, true] {
                            let layout = Layout {
                                compact,
                                narrow,
                                sidebar,
                            };
                            for slot in ["composer", "context"] {
                                for kind in [
                                    "button",
                                    "toggle",
                                    "select",
                                    "menu",
                                    "picker",
                                    "popover",
                                    "indicator",
                                ] {
                                    for overflow in ["auto", "menu"] {
                                        for order in [-100, 0, 100] {
                                            let entry: Contribution = serde_json::from_value(json!({
                                    "id":"sample", "label":{"label":"Sample"}, "slot":slot,
                                    "kind":kind,"overflow":overflow,"order":order
                                        }))
                                        .unwrap();
                                            assert_ne!(
                                                layout.includes(&entry, Form::Toolbar),
                                                layout.includes(&entry, Form::Menu),
                                                "every contribution has one destination"
                                            );
                                            assert_eq!(
                                                layout.includes(&entry, Form::Icons),
                                                layout.includes(&entry, Form::Toolbar),
                                                "every contribution has one destination"
                                            );
                                            if slot == "composer" && kind == "indicator" {
                                                assert!(
                                                    layout.includes(&entry, Form::Toolbar),
                                                    "every contribution has one destination"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }) as fn(&str),
            ),
            (
                "model_has_only_the_explicit_entry",
                (|_scenario: &str| {
                    assert!(
                        super::super::native::controls()
                            .iter()
                            .all(|entry| entry.declaration.id != "model"),
                        "model has only the explicit entry"
                    );
                }),
            ),
            (
                "other_slots_keep_their_requested_form",
                (|_scenario: &str| {
                    for slot in ["status", "model", "project", "project_menu"] {
                        let entry: Contribution = serde_json::from_value(json!({
                        "id":"sample", "label":{"label":"Sample"}, "slot":slot,
                        "kind":"button", "overflow":"menu"
                            }))
                        .unwrap();
                        for compact in [false, true] {
                            for narrow in [false, true] {
                                for sidebar in [false, true] {
                                    let layout = Layout {
                                        compact,
                                        narrow,
                                        sidebar,
                                    };
                                    for form in [
                                        Form::Toolbar,
                                        Form::Icons,
                                        Form::Menu,
                                        Form::Project,
                                        Form::Model,
                                    ] {
                                        assert!(
                                            layout.includes(&entry, form),
                                            "other slots keep their requested form"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
