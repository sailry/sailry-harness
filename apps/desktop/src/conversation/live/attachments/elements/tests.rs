use super::*;
use core::prelude::v1::test;

fn chip(key: usize, label: &str) -> Chip {
    Chip {
        key,
        label: label.into(),
        details: Details {
            selector: format!("#{label}"),
            component: String::new(),
            page: "Checkout".into(),
        },
    }
}

fn draft(key: usize, chip: Option<usize>, attachment: Option<AttachmentId>) -> Bound {
    Bound {
        key,
        chip,
        attachment,
    }
}

fn selection(id: AttachmentId, label: &str) -> Reference {
    Reference {
        target: Target::Attachment(id),
        label: label.into(),
    }
}

mod send {
    use super::*;

    #[test]
    fn keeps_only_present_chips() {
        let chips = [chip(0, "Pay"), chip(1, "Card")];
        let content = InputContent::new("Fix @Pay and @Card")
            .with_token(4..8, chips[0].token())
            .unwrap();
        assert_eq!(present(&content), vec![0]);
        let drafts = [
            draft(0, Some(0), None),
            draft(1, Some(0), None),
            draft(2, Some(1), None),
            draft(3, Some(1), None),
            draft(4, None, None),
        ];
        assert_eq!(removed(&drafts, &present(&content), &[], &[]), vec![2, 3]);
    }

    #[test]
    fn drops_removed_selections() {
        let [image, context, other] = [(); 3].map(|_| AttachmentId::new());
        let selected = vec![selection(image, "Pay"), selection(context, "Pay")];
        let drafts = [
            draft(0, None, Some(image)),
            draft(1, None, Some(context)),
            draft(2, None, Some(other)),
        ];
        assert!(removed(&drafts, &[], &selected, &selected).is_empty());
        assert_eq!(removed(&drafts, &[], &selected, &[]), vec![0, 1]);
    }

    #[test]
    fn names_published_drafts_by_chip() {
        let [image, context, unsent] = [(); 3].map(|_| AttachmentId::new());
        let chips = [chip(4, "Pay"), chip(5, "Card")];
        let drafts = [
            draft(0, Some(4), Some(image)),
            draft(1, Some(4), Some(context)),
            draft(2, Some(5), Some(unsent)),
            draft(3, None, Some(AttachmentId::new())),
        ];
        assert_eq!(
            references(&chips, &drafts, &[image, context]),
            vec![selection(image, "Pay"), selection(context, "Pay")]
        );
        assert!(references(&chips, &drafts[2..], &[image]).is_empty());
    }
}

mod recovered {
    use super::*;

    #[test]
    fn round_trips_positions() {
        let chips = vec![chip(7, "Pay"), chip(9, "Card")];
        let drafts = [
            draft(10, None, None),
            draft(11, Some(7), None),
            draft(12, Some(7), None),
            draft(13, Some(9), None),
        ];
        let recorded = positions(&chips, &drafts);
        assert_eq!(recorded[0].drafts, vec![1, 2]);
        assert_eq!(recorded[1].drafts, vec![3]);
        let (restored, bindings) = restore(recorded, drafts.len()).unwrap();
        assert_eq!(restored, chips);
        assert_eq!(bindings, vec![(1, 7), (2, 7), (3, 9)]);
    }

    #[test]
    fn rejects_inconsistent_records() {
        let chips = vec![chip(1, "Pay")];
        let drafts = [draft(0, Some(1), None), draft(1, Some(1), None)];
        let recorded = positions(&chips, &drafts);
        assert!(restore(recorded.clone(), 1).is_none());
        let mut duplicate = recorded.clone();
        duplicate.push(duplicate[0].clone());
        assert!(restore(duplicate, 2).is_none());
        let mut shared = recorded;
        shared.push(recovery::Chip {
            key: 2,
            label: "Card".into(),
            details: Details::default(),
            drafts: vec![1],
        });
        assert!(restore(shared, 2).is_none());
    }
}
