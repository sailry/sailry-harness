use super::*;
use gpui_kit::component::command::CommandItem;

pub(super) fn rows(kind: Kind, entries: &[Entry], selected: Option<usize>) -> Vec<CommandItem> {
    if selected.is_some() {
        let mut rows = kind
            .actions()
            .into_iter()
            .map(|action| {
                item(
                    tr(action.key()).to_string(),
                    String::new(),
                    action.icon(),
                    format!("connection-action-{}", action.name()),
                    false,
                )
            })
            .collect::<Vec<_>>();
        rows.push(item(
            tr("reference_back").to_string(),
            String::new(),
            IconName::ChevronLeft,
            "connection-back".into(),
            false,
        ));
        return rows;
    }
    let mut rows = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            item(
                entry.name.clone(),
                entry.detail.clone(),
                match kind {
                    Kind::Database => IconName::HardDrive,
                    Kind::Ssh => IconName::Network,
                },
                format!("connection-choice-{index}"),
                true,
            )
        })
        .collect::<Vec<_>>();
    rows.push(item(
        tr(match kind {
            Kind::Database => "db_new",
            Kind::Ssh => "ssh_new",
        })
        .to_string(),
        String::new(),
        IconName::Plus,
        "connection-add".into(),
        false,
    ));
    rows
}

fn item(
    label: String,
    detail: String,
    icon: IconName,
    selector: String,
    submenu: bool,
) -> CommandItem {
    CommandItem::new()
        .label(label.clone())
        .keywords([format!("{label} {detail}")])
        .child(move |_, cx| {
            let selector = selector.clone();
            h_flex()
                .debug_selector(move || selector.clone())
                .w_full()
                .min_w_0()
                .gap_2()
                .child(Icon::new(icon.clone()).size_4())
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .line_height(relative(1.25))
                        .child(div().truncate().child(label.clone()))
                        .when(!detail.is_empty(), |column| {
                            column.child(
                                div()
                                    .truncate()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(detail.clone()),
                            )
                        }),
                )
                .when(submenu, |row| {
                    row.child(Icon::new(IconName::ChevronRight).size_4())
                })
        })
}
