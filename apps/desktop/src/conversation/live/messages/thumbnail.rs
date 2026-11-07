//! Preview receipts show a bounded image of their revision inside the card.
//! The image comes from the shared hidden renderer; rows never embed a WebView.
use super::*;
use crate::content::visuals::snapshot::{self, Cache, Key, Snapshot, Target};
use crate::theme::css::Palette;
use sailry_protocol::artifact::Summary;

impl View {
    /// Adds the prepared image and, while connected, requests one for the
    /// card's measured width. Other receipts keep the ordinary card.
    pub(super) fn with_thumbnail(
        &self,
        card: Div,
        artifact: &Summary,
        turn: TurnId,
        cx: &mut Context<Self>,
    ) -> Div {
        if !snapshot::eligible(artifact) {
            return card;
        }
        let palette = Palette::of(cx);
        let theme = palette.digest();
        let target = Target::new(self.binding.client.target(), artifact);
        let shown = cx
            .try_global::<Cache>()
            .and_then(|cache| cache.show(&target, theme));
        let card = match shown {
            Some(prepared) => card.child(self.thumbnail(prepared, artifact, cx)),
            None => card,
        };
        if !self.connected() {
            return card;
        }
        let owner = cx.entity().downgrade();
        let artifact = artifact.clone();
        card.on_children_prepainted(move |bounds, window, cx| {
            let Some(bounds) = bounds.first() else {
                return;
            };
            let key = Key::new(
                target,
                theme,
                f32::from(bounds.size.width),
                window.scale_factor(),
            );
            if !cx
                .try_global::<Cache>()
                .is_none_or(|cache| cache.wanted(&key))
            {
                return;
            }
            let owner = owner.clone();
            let artifact = artifact.clone();
            let palette = palette.clone();
            window.defer(cx, move |window, cx| {
                request(owner, key, artifact, palette, turn, window, cx)
            });
        })
    }

    fn thumbnail(
        &self,
        prepared: Snapshot,
        artifact: &Summary,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = artifact.id;
        let revision = artifact.revision;
        let open = artifact.clone();
        let source = cx.entity().downgrade();
        div()
            .id(format!("artifact-document-thumbnail-{id}-{revision}"))
            .w_full()
            .aspect_ratio(prepared.width / prepared.height)
            .cursor_pointer()
            .border_b_1()
            .border_color(cx.theme().border)
            .aria_label(tr("artifact_open"))
            .debug_selector(move || format!("artifact-document-thumbnail-{id}-{revision}"))
            .on_click(move |_, _, cx| {
                let _ = source.update(cx, |_, cx| cx.emit(Event::Document(open.clone())));
            })
            .child(img(prepared.image).size_full().object_fit(ObjectFit::Cover))
            .into_any_element()
    }

    fn thumbnail_ready(&mut self, turn: TurnId, cx: &mut Context<Self>) {
        if let Some(index) = self.rows.iter().position(|row| *row == turn) {
            self.scroller.update(cx, |scroller, cx| {
                scroller.remeasure_items(index..index + 1, cx)
            });
        }
        cx.notify();
    }
}

/// Reads the captured revision through the ordinary local or remote command,
/// then renders it. Runs outside layout because it changes the shared cache.
fn request(
    owner: WeakEntity<View>,
    key: Key,
    artifact: Summary,
    palette: Palette,
    turn: TurnId,
    window: &mut Window,
    cx: &mut App,
) {
    if !cx
        .try_global::<Cache>()
        .is_none_or(|cache| cache.wanted(&key))
    {
        return;
    }
    let Some(view) = owner.upgrade() else {
        return;
    };
    if !view.read(cx).connected() {
        return;
    }
    let Some(read) = crate::plugins::Panel::read_document(&view, artifact, cx) else {
        return;
    };
    cx.default_global::<Cache>().begin(key);
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let result = match read.await {
            Ok(content) => {
                let rendering = cx.update(|cx| {
                    handle.update(cx, |_, window, cx| {
                        snapshot::render(content.html, palette, key, window, cx)
                    })
                });
                match rendering {
                    Ok(task) => task.await,
                    Err(_) => Err("Snapshot window closed".into()),
                }
            }
            Err(error) => Err(error.to_owned()),
        };
        cx.update(|cx| {
            cx.default_global::<Cache>().finish(key, result);
            let _ = owner.update(cx, |view, cx| view.thumbnail_ready(turn, cx));
        });
    })
    .detach();
}
