use super::*;

impl Editor {
    pub(super) fn appearance_picker(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        super::super::appearance::picker(
            "project",
            cx.entity().downgrade(),
            super::super::appearance::Selection {
                selected: self.appearance.clone(),
                read: |editor: &Editor| editor.appearance.clone(),
                write: |editor, value| editor.appearance = value,
            },
            self.pending,
            tr("project_appearance"),
            cx,
        )
    }
}
