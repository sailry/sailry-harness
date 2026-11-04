use sailry_protocol::plugin::{Category, skillcatalog};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Skills {
    #[default]
    All,
    Official,
    Directory,
    Hub,
}

impl Skills {
    pub fn source(self) -> skillcatalog::Source {
        match self {
            Self::All | Self::Official => skillcatalog::Source::Official,
            Self::Directory => skillcatalog::Source::SkillsSh,
            Self::Hub => skillcatalog::Source::Skillhub,
        }
    }
}

#[derive(Default)]
pub(super) struct Filter {
    pub category: Option<Category>,
    pub installed: bool,
    pub skills: Skills,
    pub dark: Option<bool>,
}

impl Filter {
    pub fn plugin(&self, category: Option<Category>, installed: bool) -> bool {
        (!self.installed || installed) && self.category.is_none_or(|value| category == Some(value))
    }

    pub fn theme(&self, modes: &[String], installed: bool) -> bool {
        (!self.installed || installed)
            && self.dark.is_none_or(|dark| {
                modes.is_empty()
                    || modes
                        .iter()
                        .any(|mode| mode == if dark { "dark" } else { "light" })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_choices() {
        for (scenario, check) in [
            (
                "combines_category_and_installed_filters",
                (|_scenario: &str| {
                    let mut filter = Filter::default();
                    assert!(
                        filter.plugin(None, false),
                        "combines category and installed filters"
                    );
                    filter.category = Some(Category::Development);
                    assert!(
                        !filter.plugin(None, true),
                        "combines category and installed filters"
                    );
                    assert!(
                        !filter.plugin(Some(Category::Data), true),
                        "combines category and installed filters"
                    );
                    assert!(
                        filter.plugin(Some(Category::Development), false),
                        "combines category and installed filters"
                    );
                    filter.installed = true;
                    assert!(
                        !filter.plugin(Some(Category::Development), false),
                        "combines category and installed filters"
                    );
                    assert!(
                        filter.plugin(Some(Category::Development), true),
                        "combines category and installed filters"
                    );
                }) as fn(&str),
            ),
            (
                "preserves_unknown_theme_modes",
                (|_scenario: &str| {
                    let filter = Filter {
                        dark: Some(true),
                        ..Default::default()
                    };
                    assert!(filter.theme(&[], false), "preserves unknown theme modes");
                    assert!(
                        filter.theme(&["light".into(), "dark".into()], false),
                        "preserves unknown theme modes"
                    );
                    assert!(
                        !filter.theme(&["light".into()], true),
                        "preserves unknown theme modes"
                    );
                    assert!(
                        filter.theme(&["dark".into()], false),
                        "preserves unknown theme modes"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
