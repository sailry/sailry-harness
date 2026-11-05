//! Link and image editing anchored in the rendered document.
//!
//! Address entry uses Kit's Popover and Input; Escape dismisses the popover and
//! returns focus to the document. Opening links is delegated to the caller.
use super::{Mode, State};
use crate::markdown::{
    BlockKind, Cursor, Doc, Mark, Part, Selection,
    doc::{MarkSpan, Text},
    render::OnLink,
};
use gpui_kit::base::Popover;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Sizable as _, ThemeStyled as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Escape, Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, FocusHandle, Focusable as _, IntoElement,
    Modifiers, MouseButton, Pixels, Point, SharedString, Window, div, point, prelude::*, px,
};
use std::{ops::Range, rc::Rc};

/// Asks the caller for an image path and reports the chosen address.
pub type OnChoose = Rc<dyn Fn(Rc<dyn Fn(String, &mut Window, &mut App)>, &mut Window, &mut App)>;

/// Renders a picture as [`crate::markdown::render::OnImage`] does, told whether
/// a click may preview it. While the document is editable a click edits the
/// image instead, so the picture must not claim the click for itself.
pub type OnImage =
    Rc<dyn Fn(&str, &[&str], usize, crate::markdown::render::Stated, bool, &App) -> AnyElement>;

pub(super) enum Popup {
    Link {
        selection: Selection,
        input: Entity<InputState>,
    },
    Image {
        block: usize,
        address: Entity<InputState>,
        caption: Entity<InputState>,
    },
}

/// Whether pasted text is a single address that can become a link.
pub(super) fn address(text: &str) -> bool {
    !text.is_empty()
        && !text.contains(char::is_whitespace)
        && ["http://", "https://", "mailto:"]
            .iter()
            .any(|scheme| text.starts_with(scheme) && text.len() > scheme.len())
}

/// The link span around a caret and its address.
pub(super) fn under(doc: &Doc, at: Cursor) -> Option<(Range<usize>, String)> {
    let text = doc.blocks.get(at.block)?.text_at(at.part)?;
    text.marks.iter().find_map(|span| match &span.mark {
        Mark::Link(url)
            if !span.range.is_empty()
                && span.range.start <= at.offset
                && at.offset <= span.range.end =>
        {
            Some((span.range.clone(), url.clone()))
        }
        _ => None,
    })
}

/// Set or remove a link over every text part a selection covers.
pub(super) fn set_link(doc: &mut Doc, selection: Selection, url: Option<&str>) {
    for (at, range) in doc.spans(selection) {
        if range.is_empty() || matches!(at.part, Part::Code | Part::Caption) {
            continue;
        }
        doc.edit_at(at, |text| relink(text, range, url));
    }
}

fn relink(text: &mut Text, range: Range<usize>, url: Option<&str>) {
    let mut marks = Vec::with_capacity(text.marks.len() + 1);
    for span in std::mem::take(&mut text.marks) {
        let overlaps = matches!(span.mark, Mark::Link(_))
            && span.range.start < range.end
            && span.range.end > range.start;
        if !overlaps {
            marks.push(span);
            continue;
        }
        if span.range.start < range.start {
            marks.push(MarkSpan {
                range: span.range.start..range.start,
                mark: span.mark.clone(),
            });
        }
        if span.range.end > range.end {
            marks.push(MarkSpan {
                range: range.end..span.range.end,
                mark: span.mark,
            });
        }
    }
    if let Some(url) = url {
        marks.push(MarkSpan {
            range,
            mark: Mark::Link(url.to_owned()),
        });
    }
    text.marks = marks;
    text.normalize_marks();
}

fn span(at: Cursor, range: &Range<usize>) -> Selection {
    Selection::new(
        Cursor::new(at.block, at.part, range.start),
        Cursor::new(at.block, at.part, range.end),
    )
}

impl State {
    /// Open links the document cannot resolve itself, such as workspace paths.
    pub fn set_link_handler(&mut self, handler: OnLink) {
        self.on_link = Some(handler);
    }

    /// Offer a file chooser in the image popover.
    pub fn set_image_chooser(&mut self, chooser: OnChoose) {
        self.on_choose = Some(chooser);
    }

    /// Document clicks place the caret; the platform modifier also opens links.
    pub(super) fn link_click(&self, cx: &mut Context<Self>) -> OnLink {
        let owner = cx.weak_entity();
        Rc::new(
            move |url: &SharedString, modifiers: Modifiers, window: &mut Window, cx: &mut App| {
                if modifiers.secondary(cx) {
                    let url = url.to_string();
                    let _ = owner.update(cx, |state, cx| state.open_link(&url, window, cx));
                }
            },
        )
    }

    pub(super) fn open_link(&mut self, url: &str, window: &mut Window, cx: &mut Context<Self>) {
        let url = SharedString::from(url.to_owned());
        if let Some(handler) = self.on_link.clone() {
            // The platform modifier is the editor's open gesture, not a request
            // to leave the application.
            handler(&url, Modifiers::default(), window, cx);
        } else if url.contains("://") || url.starts_with("mailto:") {
            cx.open_url(&url);
        }
    }

    pub(super) fn close_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.popup.take().is_some() {
            self.popup_events.clear();
            self.focus.focus(window, cx);
            cx.notify();
        }
    }

    pub(super) fn edit_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly || self.mode != Mode::Document {
            return;
        }
        self.cancel_composition(cx);
        let at = self.cursor();
        let existing = under(&self.doc, at);
        let selection = match &existing {
            Some((range, _)) if self.selection.is_collapsed() => span(at, range),
            _ => self.selection,
        };
        let url = existing.map(|(_, url)| url).unwrap_or_default();
        let placeholder = cx.global::<crate::Labels>().link_address.clone();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(url)
        });
        let events = cx.subscribe_in(
            &input,
            window,
            |state, input, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    let url = input.read(cx).value().trim().to_owned();
                    state.commit_link(url, window, cx);
                }
            },
        );
        self.popup = Some(Popup::Link { selection, input });
        self.popup_events = vec![events];
        cx.notify();
    }

    fn commit_link(&mut self, url: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Popup::Link { selection, .. }) = self.popup.take() else {
            return;
        };
        self.popup_events.clear();
        self.focus.focus(window, cx);
        if self.readonly {
            cx.notify();
            return;
        }
        let selection = selection.clamp(&self.doc);
        let before = self.doc.clone();
        if url.is_empty() {
            set_link(&mut self.doc, selection, None);
            self.selection = Selection::at(selection.ordered().1);
        } else if selection.is_collapsed() {
            let splice = self.doc.replace(selection, Text::link(&url));
            self.place(splice.caret);
        } else {
            set_link(&mut self.doc, selection, Some(&url));
            self.selection = Selection::at(selection.ordered().1);
        }
        self.apply(before, window, cx);
    }

    fn unlink(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.cursor();
        let Some((range, _)) = under(&self.doc, at) else {
            return;
        };
        let before = self.doc.clone();
        set_link(&mut self.doc, span(at, &range), None);
        self.focus.focus(window, cx);
        self.apply(before, window, cx);
    }

    /// Replace a just-pasted address with the text it replaced, linked to it.
    /// The plain paste stays a separate undo step.
    pub(super) fn link_paste(
        &mut self,
        start: Cursor,
        url: &str,
        original: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let end = start.offset + url.len();
        let pasted = self
            .doc
            .blocks
            .get(start.block)
            .and_then(|block| block.text_at(start.part))
            .is_some_and(|text| text.text.get(start.offset..end) == Some(url));
        if !pasted {
            return;
        }
        let before = self.doc.clone();
        self.doc.edit_at(start, |text| {
            text.remove(start.offset..end);
            text.insert(start.offset, original);
            relink(text, start.offset..start.offset + original.len(), Some(url));
        });
        self.place(Cursor::new(
            start.block,
            start.part,
            start.offset + original.len(),
        ));
        self.apply(before, window, cx);
    }

    pub(super) fn edit_image(&mut self, block: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.readonly || self.mode != Mode::Document {
            return;
        }
        let Some((url, alt)) = self
            .doc
            .blocks
            .get(block)
            .and_then(|value| match &value.kind {
                BlockKind::Image { url, alt, .. } => Some((url.clone(), alt.text.clone())),
                _ => None,
            })
        else {
            return;
        };
        self.cancel_composition(cx);
        let labels = cx.global::<crate::Labels>();
        let (address_label, caption_label) =
            (labels.image_address.clone(), labels.image_alt.clone());
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(address_label)
                .default_value(url)
        });
        let caption = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(caption_label)
                .default_value(alt)
        });
        let mut events = Vec::with_capacity(2);
        for input in [&address, &caption] {
            events.push(cx.subscribe_in(
                input,
                window,
                |state, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        state.commit_image(window, cx);
                    }
                },
            ));
        }
        self.popup = Some(Popup::Image {
            block,
            address,
            caption,
        });
        self.popup_events = events;
        cx.notify();
    }

    fn commit_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Popup::Image {
            block,
            address,
            caption,
        }) = self.popup.take()
        else {
            return;
        };
        self.popup_events.clear();
        let url = address.read(cx).value().trim().to_owned();
        let alt = caption.read(cx).value().to_string();
        self.focus.focus(window, cx);
        if self.readonly {
            cx.notify();
            return;
        }
        let before = self.doc.clone();
        if let Some(BlockKind::Image {
            url: target,
            alt: text,
            ..
        }) = self.doc.blocks.get_mut(block).map(|value| &mut value.kind)
        {
            *target = url;
            *text = Text::plain(alt);
            crate::markdown::parse::collapse_to_one_line(text);
        }
        self.apply(before, window, cx);
    }

    fn choose_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chooser) = self.on_choose.clone() else {
            return;
        };
        let owner = cx.weak_entity();
        chooser(
            Rc::new(move |path: String, window: &mut Window, cx: &mut App| {
                let _ = owner.update(cx, |state, cx| {
                    if let Some(Popup::Image { address, .. }) = &state.popup {
                        address.update(cx, |input, cx| input.set_value(path.clone(), window, cx));
                    }
                });
            }),
            window,
            cx,
        );
    }

    /// The open popover, or the bar for a link under the caret.
    pub(super) fn overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.mode != Mode::Document || self.readonly {
            return None;
        }
        let origin = self.scroll.bounds().origin;
        match &self.popup {
            Some(Popup::Link { selection, input }) => {
                let (at, height) = self.layouts.position(selection.head)?;
                let body = v_flex().w(px(320.)).child(Input::new(input));
                let focus = input.focus_handle(cx);
                Some(self.popover(
                    "markdown-link-popover",
                    at - origin + point(px(0.), height),
                    body.into_any_element(),
                    focus,
                    cx,
                ))
            }
            Some(Popup::Image {
                block,
                address,
                caption,
            }) => {
                let bounds = self.layouts.block_bounds(*block)?;
                let chooser = self.on_choose.is_some().then(|| {
                    Button::new("markdown-image-choose")
                        .small()
                        .label(cx.global::<crate::Labels>().image_choose.clone())
                        .on_click(
                            cx.listener(|state, _, window, cx| state.choose_image(window, cx)),
                        )
                });
                let body = v_flex()
                    .w(px(320.))
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().flex_1().min_w_0().child(Input::new(address)))
                            .children(chooser),
                    )
                    .child(Input::new(caption));
                let focus = address.focus_handle(cx);
                Some(self.popover(
                    "markdown-image-popover",
                    bounds.bottom_left() - origin,
                    body.into_any_element(),
                    focus,
                    cx,
                ))
            }
            None => self.link_bar(origin, cx),
        }
    }

    fn popover(
        &self,
        id: &'static str,
        at: Point<Pixels>,
        body: AnyElement,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let owner = cx.weak_entity();
        let escape = owner.clone();
        Popover::new(id)
            .absolute()
            .left(at.x)
            .top(at.y)
            .open(true)
            .track_focus(&focus)
            .trigger_with(|_, _, _| div().size_0().into_any_element())
            .on_open_change(move |open, window, cx| {
                if !*open {
                    let owner = owner.clone();
                    window.defer(cx, move |window, cx| {
                        let _ = owner.update(cx, |state, cx| state.close_popup(window, cx));
                    });
                }
            })
            .content(move |_, _, cx| {
                v_flex()
                    .popover_style(cx)
                    .p_2()
                    .gap_2()
                    .on_action(move |_: &Escape, window, cx| {
                        let _ = escape.update(cx, |state, cx| state.close_popup(window, cx));
                    })
                    .child(body)
            })
            .into_any_element()
    }

    fn link_bar(&self, origin: Point<Pixels>, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.selection.is_collapsed() || self.dragging {
            return None;
        }
        let at = self.cursor();
        let (_, url) = under(&self.doc, at)?;
        let (position, height) = self.layouts.position(at)?;
        let position = position - origin + point(px(0.), height + px(4.));
        let labels = cx.global::<crate::Labels>();
        let actions = [
            (
                "markdown-link-open",
                "external-link",
                labels.link_open.clone(),
            ),
            ("markdown-link-edit", "pencil", labels.link_edit.clone()),
            ("markdown-link-copy", "copy", labels.link_copy.clone()),
            ("markdown-link-remove", "unlink", labels.link_remove.clone()),
        ];
        let muted = cx.theme().muted_foreground;
        let buttons = actions.map(|(id, icon, label)| {
            let url = url.clone();
            Button::new(id)
                .ghost()
                .small()
                .icon(Icon::empty().path(format!("icons/{icon}.svg")))
                .tooltip(label.clone())
                .accessibility_label(label)
                .on_click(cx.listener(move |state, _, window, cx| match id {
                    "markdown-link-open" => state.open_link(&url, window, cx),
                    "markdown-link-edit" => state.edit_link(window, cx),
                    "markdown-link-copy" => {
                        cx.write_to_clipboard(ClipboardItem::new_string(url.clone()))
                    }
                    _ => state.unlink(window, cx),
                }))
        });
        Some(
            h_flex()
                .id("markdown-link-bar")
                .absolute()
                .left(position.x)
                .top(position.y)
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .popover_style(cx)
                .p_1()
                .gap_1()
                .child(
                    div()
                        .px_1()
                        .max_w(px(220.))
                        .truncate()
                        .text_sm()
                        .text_color(muted)
                        .child(url),
                )
                .children(buttons)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;

    fn linked(doc: &Doc) -> Vec<(Range<usize>, String)> {
        let text = doc.blocks[0].text_at(Part::Body).unwrap();
        text.marks
            .iter()
            .filter_map(|span| match &span.mark {
                Mark::Link(url) => Some((span.range.clone(), url.clone())),
                _ => None,
            })
            .collect()
    }

    fn select(start: usize, end: usize) -> Selection {
        Selection::new(
            Cursor::new(0, Part::Body, start),
            Cursor::new(0, Part::Body, end),
        )
    }

    #[test]
    fn preserves_document_behavior() {
        for (scenario, check) in [
            (
                "address_accepts_single_urls",
                (|scenario: &'static str| {
                    assert!(address("https://example.com/a?b=c"), "{scenario}");
                    assert!(address("mailto:team@example.com"), "{scenario}");
                    assert!(!address("https://"), "{scenario}");
                    assert!(!address("docs/node.md"), "{scenario}");
                    assert!(!address("see https://example.com"), "{scenario}");
                }) as fn(&'static str),
            ),
            (
                "finds_caret_link",
                (|scenario: &'static str| {
                    let doc = parse("see [docs](a.md)");
                    assert_eq!(
                        under(&doc, Cursor::new(0, Part::Body, 6)),
                        Some((4..8, "a.md".to_owned())),
                        "{scenario}"
                    );
                    assert_eq!(
                        under(&doc, Cursor::new(0, Part::Body, 1)),
                        None,
                        "{scenario}"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }

    mod set_link {
        use super::*;

        #[test]
        fn edits_selected_addresses() {
            for (case, source, range, address, expected, text) in [
                (
                    "new",
                    "read the docs",
                    9..13,
                    Some("docs/node.md"),
                    vec![(9..13, "docs/node.md".to_owned())],
                    "read the docs",
                ),
                (
                    "replace",
                    "[docs](a.md)",
                    0..4,
                    Some("b.md"),
                    vec![(0..4, "b.md".to_owned())],
                    "docs",
                ),
                (
                    "remove",
                    "[docs](a.md) here",
                    0..4,
                    None,
                    vec![],
                    "docs here",
                ),
            ] {
                let mut doc = parse(source);
                set_link(&mut doc, select(range.start, range.end), address);
                assert_eq!(linked(&doc), expected, "{case}: links");
                assert_eq!(
                    doc.blocks[0].text_at(Part::Body).unwrap().text,
                    text,
                    "{case}: text"
                );
            }
        }
    }
}
