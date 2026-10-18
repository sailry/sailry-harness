use sailry_protocol::plugin::Category;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Skills {
    #[default]
    All,
    Official,
    Directory,
    Hub,
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
    fn combines_category_and_installed_filters() {
        let mut filter = Filter::default();
        assert!(filter.plugin(None, false));
        filter.category = Some(Category::Development);
        assert!(!filter.plugin(None, true));
        assert!(!filter.plugin(Some(Category::Data), true));
        assert!(filter.plugin(Some(Category::Development), false));
        filter.installed = true;
        assert!(!filter.plugin(Some(Category::Development), false));
        assert!(filter.plugin(Some(Category::Development), true));
    }

    #[test]
    fn preserves_unknown_theme_modes() {
        let filter = Filter {
            dark: Some(true),
            ..Default::default()
        };
        assert!(filter.theme(&[], false));
        assert!(filter.theme(&["light".into(), "dark".into()], false));
        assert!(!filter.theme(&["light".into()], true));
        assert!(filter.theme(&["dark".into()], false));
    }
}
