use super::*;
use crate::tr;
use sailry_protocol::Command;

pub(super) fn model(owner: Entity<Panel>, model: Model, window: &mut Window, cx: &mut App) {
    let title = tr("knowledge_delete");
    let detail = rust_i18n::t!(
        "knowledge_delete_model_effect",
        name = owner.read(cx).model_name(model)
    );
    crate::prompts::confirm(
        &title,
        &detail,
        tr("knowledge_delete"),
        window,
        cx,
        move |window, cx| {
            owner.update(cx, |panel, cx| {
                panel.submit(Command::DeleteKnowledgeModel { model }, window, cx)
            });
        },
    );
}

pub(super) fn collection(
    owner: Entity<Panel>,
    collection: Collection,
    window: &mut Window,
    cx: &mut App,
) {
    let title = tr("knowledge_delete");
    let detail = rust_i18n::t!(
        "knowledge_delete_collection_effect",
        name = collection.definition.name
    );
    crate::prompts::confirm(
        &title,
        &detail,
        tr("knowledge_delete"),
        window,
        cx,
        move |window, cx| {
            owner.update(cx, |panel, cx| {
                panel.submit(
                    Command::DeleteKnowledgeCollection {
                        id: collection.id,
                        expected_revision: collection.revision,
                    },
                    window,
                    cx,
                )
            });
        },
    );
}
