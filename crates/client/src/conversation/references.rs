//! Attachment references bind attachments to the selection presented by their label.
use sailry_protocol::{
    AttachmentId,
    conversation::reference::{Reference, Target},
};

fn targets(references: &[Reference], id: AttachmentId) -> bool {
    references
        .iter()
        .any(|reference| reference.target == Target::Attachment(id))
}

/// Attachments of one selection in reference order.
pub fn selection(references: &[Reference], label: &str) -> Vec<AttachmentId> {
    references
        .iter()
        .filter(|reference| reference.label == label)
        .filter_map(|reference| match reference.target {
            Target::Attachment(id) => Some(id),
            _ => None,
        })
        .collect()
}

/// An edited input keeps an attachment unless every reference selecting it was removed.
pub fn retained(
    attachments: &[AttachmentId],
    previous: &[Reference],
    current: &[Reference],
) -> Vec<AttachmentId> {
    attachments
        .iter()
        .copied()
        .filter(|id| !targets(previous, *id) || targets(current, *id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(target: Target, label: &str) -> Reference {
        Reference {
            target,
            label: label.into(),
        }
    }

    #[test]
    fn drops_removed_selections() {
        let [image, context, other] = [(); 3].map(|_| AttachmentId::new());
        let previous = vec![
            reference(Target::Attachment(image), "Button"),
            reference(Target::Attachment(context), "Button"),
            reference(Target::File("a.rs".into()), "a.rs"),
        ];
        let attachments = [image, context, other];
        assert_eq!(
            retained(&attachments, &previous, &previous),
            attachments.to_vec()
        );
        assert_eq!(
            retained(&attachments, &previous, &previous[2..]),
            vec![other]
        );
        assert_eq!(
            retained(&attachments, &previous, &previous[1..]),
            vec![context, other]
        );
    }

    #[test]
    fn groups_by_label() {
        let [image, context, other] = [(); 3].map(|_| AttachmentId::new());
        let references = vec![
            reference(Target::Attachment(image), "Button"),
            reference(Target::Host, "Button"),
            reference(Target::Attachment(other), "Card"),
            reference(Target::Attachment(context), "Button"),
        ];
        assert_eq!(selection(&references, "Button"), vec![image, context]);
        assert!(selection(&references, "Missing").is_empty());
    }
}
