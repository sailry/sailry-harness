use sailry_protocol::plugin::{self, Extension};

pub(super) fn valid(extension: &Extension) -> bool {
    extension.cards.len() <= plugin::card::MAX_CARDS
        && extension.cards.iter().enumerate().all(|(index, card)| {
            card.valid(extension)
                && !extension.cards[..index]
                    .iter()
                    .any(|other| other.key == card.key)
                && jsonschema::draft202012::options()
                    .build(&card.props_schema)
                    .is_ok()
        })
}
