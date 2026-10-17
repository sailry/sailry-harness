use std::{
    cmp::Ordering,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default)]
pub(super) struct Order {
    pub column: usize,
    pub descending: bool,
}

pub(super) struct Entry<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub directory: bool,
    pub size: u64,
}

impl Order {
    pub fn compare(&self, left: &Entry<'_>, right: &Entry<'_>) -> Ordering {
        let folders = right.directory.cmp(&left.directory);
        if folders != Ordering::Equal {
            return folders;
        }
        let value = match self.column {
            1 => left.kind.cmp(right.kind),
            2 => left.size.cmp(&right.size),
            _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        }
        .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        .then_with(|| left.name.cmp(right.name));
        if self.descending {
            value.reverse()
        } else {
            value
        }
    }
}

#[derive(Default)]
pub(super) struct Search {
    prefix: String,
    last: Option<Instant>,
}

impl Search {
    pub fn reset(&mut self) {
        self.prefix.clear();
        self.last = None;
    }

    pub fn find(
        &mut self,
        text: &str,
        now: Instant,
        names: &[&str],
        selected: Option<usize>,
    ) -> Option<usize> {
        if text.is_empty() || text.chars().any(char::is_control) || names.is_empty() {
            return None;
        }
        if self
            .last
            .is_none_or(|last| now.duration_since(last) > Duration::from_secs(1))
        {
            self.prefix.clear();
        }
        let text = text.to_lowercase();
        let cycle = self.prefix == text && text.chars().count() == 1;
        let extending = !self.prefix.is_empty() && !cycle;
        if !cycle {
            self.prefix.push_str(&text);
        }
        self.last = Some(now);
        let start = selected
            .map(|index| index + usize::from(!extending))
            .unwrap_or(0);
        (0..names.len())
            .map(|offset| (start + offset) % names.len())
            .find(|index| names[*index].to_lowercase().starts_with(&self.prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_stay_first_in_both_directions() {
        let folder = Entry {
            name: "z",
            kind: "Folder",
            directory: true,
            size: 0,
        };
        let file = Entry {
            name: "a",
            kind: "Text",
            directory: false,
            size: 100,
        };
        for column in 0..3 {
            for descending in [false, true] {
                assert_eq!(
                    Order { column, descending }.compare(&folder, &file),
                    Ordering::Less
                );
            }
        }
    }

    #[test]
    fn columns_use_names_types_and_numeric_sizes() {
        let left = Entry {
            name: "beta",
            kind: "Image",
            directory: false,
            size: 2,
        };
        let right = Entry {
            name: "Alpha",
            kind: "Text",
            directory: false,
            size: 100,
        };
        for (column, expected) in [
            (0, Ordering::Greater),
            (1, Ordering::Less),
            (2, Ordering::Less),
        ] {
            assert_eq!(
                Order {
                    column,
                    descending: false
                }
                .compare(&left, &right),
                expected
            );
            assert_eq!(
                Order {
                    column,
                    descending: true
                }
                .compare(&left, &right),
                expected.reverse()
            );
        }
    }

    #[test]
    fn prefixes_extend_and_repeated_letters_cycle() {
        let mut search = Search::default();
        let now = Instant::now();
        let names = ["Alpha", "Apple", "Beta"];
        assert_eq!(search.find("a", now, &names, None), Some(0));
        assert_eq!(search.find("a", now, &names, Some(0)), Some(1));
        assert_eq!(search.find("p", now, &names, Some(1)), Some(1));
        assert_eq!(search.find("p", now, &names, Some(1)), Some(1));
        assert_eq!(
            search.find("b", now + Duration::from_secs(2), &names, Some(1)),
            Some(2)
        );
        search.reset();
        assert_eq!(search.find("A", now, &names, Some(1)), Some(0));
        assert_eq!(search.find("\n", now, &names, None), None);
    }
}
