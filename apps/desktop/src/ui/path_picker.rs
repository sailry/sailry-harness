//! Local file and directory selection through the OS dialog or an external drop.
//!
//! GPUI Kit 0.7.3 has no file input or drop zone: `Attachment` renders composer
//! chips and `Button` only clicks. The picker composes Kit `Button`, `Icon`,
//! `Spinner` and `Tag` with GPUI's `ExternalPaths` drop events and theme tokens.
//! It only selects paths and shows the caller's selection; callers own uploads,
//! validation results and state.
use crate::tr;
use gpui_kit::{
    assets::IconName,
    component::{
        ActiveTheme as _, Disableable as _, Icon, Sizable as _, StyledExt as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        notification::Notification,
        spinner::Spinner,
        tag::Tag,
        v_flex,
    },
    prelude::FluentBuilder as _,
    *,
};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
};

const ZONE_HEIGHT: f32 = 164.;
const FIELD_HEIGHT: f32 = 36.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    #[allow(dead_code, reason = "local skill imports adopt directory selection")]
    Directory,
    File,
}

impl Kind {
    fn icon(self, archive: bool) -> IconName {
        match self {
            Self::Directory => IconName::Folder,
            Self::File if archive => IconName::FileArchive,
            Self::File => IconName::File,
        }
    }

    fn tint(self, cx: &App) -> Hsla {
        match self {
            Self::Directory => cx.theme().blue,
            Self::File => cx.theme().magenta,
        }
    }
}

/// The caller's current selection.
pub(crate) struct Picked {
    pub path: PathBuf,
    /// Size, item count or another short fact the caller already knows.
    pub detail: Option<SharedString>,
}

impl Picked {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, detail: None }
    }

    fn name(&self) -> SharedString {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .into_owned()
            .into()
    }

    fn meta(&self) -> SharedString {
        let path = self.path.display().to_string();
        match &self.detail {
            Some(detail) => format!("{path} · {detail}").into(),
            None => path.into(),
        }
    }
}

/// Whether a path fits the picker: its kind and, for files, an accepted
/// extension such as `zip` or `tar.gz`. An empty list accepts every file.
pub(crate) fn accepts(kind: Kind, accept: &[&str], path: &Path, directory: bool) -> bool {
    match kind {
        Kind::Directory => directory,
        Kind::File if directory => false,
        Kind::File if accept.is_empty() => true,
        Kind::File => path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                let name = name.to_lowercase();
                accept.iter().any(|extension| {
                    name.strip_suffix(&extension.to_lowercase())
                        .and_then(|stem| stem.strip_suffix('.'))
                        .is_some_and(|stem| !stem.is_empty())
                })
            }),
    }
}

type PickHandler = Rc<dyn Fn(&PathBuf, &mut Window, &mut App)>;
type ClearHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub(crate) struct PathPicker {
    id: SharedString,
    kind: Kind,
    accept: &'static [&'static str],
    label: &'static str,
    target: Option<SharedString>,
    value: Option<Picked>,
    busy: bool,
    compact: bool,
    disabled: bool,
    on_pick: Option<PickHandler>,
    on_clear: Option<ClearHandler>,
}

impl PathPicker {
    pub(crate) fn new(id: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            kind: Kind::File,
            accept: &[],
            label: "path_picker_choose",
            target: None,
            value: None,
            busy: false,
            compact: false,
            disabled: false,
            on_pick: None,
            on_clear: None,
        }
    }

    pub(crate) fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }

    /// Accepted file extensions without the leading dot.
    #[allow(dead_code, reason = "local skill imports adopt archive filters")]
    pub(crate) fn accept(mut self, accept: &'static [&'static str]) -> Self {
        self.accept = accept;
        self
    }

    /// The localization key for the empty title and the dialog prompt.
    pub(crate) fn label(mut self, key: &'static str) -> Self {
        self.label = key;
        self
    }

    /// The host that receives an upload, shown under the drop zone.
    #[allow(dead_code, reason = "local skill imports adopt the upload target")]
    pub(crate) fn target(mut self, host: Option<SharedString>) -> Self {
        self.target = host;
        self
    }

    pub(crate) fn value(mut self, value: Option<Picked>) -> Self {
        self.value = value;
        self
    }

    /// Work without measurable progress; shows a spinner and blocks picking.
    pub(crate) fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    /// A single form row instead of the drop zone and selection card.
    pub(crate) fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    pub(crate) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Receives a chosen or dropped path that fits the kind and accepted types.
    pub(crate) fn on_pick(
        mut self,
        handler: impl Fn(&PathBuf, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_pick = Some(Rc::new(handler));
        self
    }

    pub(crate) fn on_clear(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_clear = Some(Rc::new(handler));
        self
    }

    fn child_id(&self, suffix: &str) -> ElementId {
        SharedString::from(format!("{}-{suffix}", self.id)).into()
    }

    fn interactive(&self) -> bool {
        !self.disabled && !self.busy && self.on_pick.is_some()
    }

    fn browse(&self) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let kind = self.kind;
        let label = self.label;
        let on_pick = self.on_pick.clone();
        move |_, window, cx| {
            let Some(on_pick) = on_pick.clone() else {
                return;
            };
            let selected = cx.prompt_for_paths(PathPromptOptions {
                files: kind == Kind::File,
                directories: kind == Kind::Directory,
                multiple: false,
                prompt: Some(tr(label)),
            });
            window
                .spawn(cx, async move |cx| {
                    let selected = selected.await;
                    let _ = cx.update(|window, cx| match selected {
                        Ok(Ok(Some(paths))) => {
                            if let Some(path) = paths.first() {
                                on_pick(path, window, cx);
                            }
                        }
                        Ok(Ok(None)) => {}
                        _ => warn("files_upload_picker", window, cx),
                    });
                })
                .detach();
        }
    }

    fn tile(&self, size: f32, cx: &App) -> Div {
        let tint = self.kind.tint(cx);
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(size))
            .rounded(cx.theme().radius_lg)
            .bg(tint.opacity(0.16))
            .text_color(tint)
            .child(if self.busy {
                Spinner::new().into_any_element()
            } else {
                Icon::new(self.kind.icon(!self.accept.is_empty()))
                    .size(px(size * 0.45))
                    .into_any_element()
            })
    }

    fn zone(&self, over: Option<bool>, cx: &App) -> Button {
        let muted = cx.theme().muted_foreground;
        let title = match over {
            Some(true) => tr("path_picker_release"),
            Some(false) => tr("path_picker_unsupported"),
            None => tr(self.label),
        };
        Button::new(self.child_id("zone"))
            .outline()
            .w_full()
            .h(px(ZONE_HEIGHT))
            .rounded(cx.theme().radius_lg)
            .border_dashed()
            .accessibility_label(tr(self.label))
            .disabled(self.disabled || self.busy)
            .on_click(self.browse())
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .when(self.disabled, |content| content.opacity(0.5))
                    .child(self.tile(44., cx))
                    .child(div().text_sm().font_medium().child(title))
                    .when(over.is_none(), |content| {
                        content.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(tr("path_picker_drop_hint")),
                        )
                    })
                    .when(over != Some(true) && !self.accept.is_empty(), |content| {
                        content.child(h_flex().gap_1().children(self.accept.iter().map(
                            |extension| Tag::secondary().small().child(format!(".{extension}")),
                        )))
                    }),
            )
    }

    fn card(&self, picked: &Picked, cx: &App) -> Stateful<Div> {
        h_flex()
            .id(self.child_id("card"))
            .w_full()
            .min_w_0()
            .gap_3()
            .p_3()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().input_background())
            .when(self.disabled, |card| card.opacity(0.5))
            .child(self.tile(40., cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .truncate()
                            .child(picked.name()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(picked.meta()),
                    ),
            )
            .when(!self.busy && self.on_pick.is_some(), |card| {
                card.child(
                    Button::new(self.child_id("replace"))
                        .outline()
                        .small()
                        .label(tr("path_picker_replace"))
                        .disabled(self.disabled)
                        .on_click(self.browse()),
                )
            })
            .children(self.clear_button())
    }

    fn field(&self, cx: &App) -> Stateful<Div> {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id(self.child_id("field"))
            .w_full()
            .min_w_0()
            .h(px(FIELD_HEIGHT))
            .gap_2()
            .pl_2p5()
            .pr_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().input)
            .bg(cx.theme().input_background())
            .when(self.disabled, |field| field.opacity(0.5))
            .child(if self.busy {
                Spinner::new().small().into_any_element()
            } else {
                Icon::new(self.kind.icon(!self.accept.is_empty()))
                    .small()
                    .text_color(muted)
                    .into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .map(|text| match &self.value {
                        Some(picked) => text.child(picked.path.display().to_string()),
                        None => text.text_color(muted).child(tr("path_picker_empty")),
                    }),
            )
            .when(self.on_pick.is_some(), |field| {
                field.child(
                    Button::new(self.child_id("choose"))
                        .ghost()
                        .small()
                        .label(tr(if self.value.is_some() {
                            "path_picker_replace"
                        } else {
                            "path_picker_choose"
                        }))
                        .accessibility_label(tr(self.label))
                        .disabled(self.disabled || self.busy)
                        .on_click(self.browse()),
                )
            })
            .when(self.value.is_some(), |field| {
                field.children(self.clear_button())
            })
    }

    fn clear_button(&self) -> Option<Button> {
        let clear = self.on_clear.clone()?;
        Some(
            Button::new(self.child_id("clear"))
                .ghost()
                .small()
                .icon(IconName::X)
                .accessibility_label(tr("path_picker_remove"))
                .disabled(self.disabled || self.busy)
                .on_click(move |event, window, cx| clear(event, window, cx)),
        )
    }
}

impl RenderOnce for PathPicker {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let over = window.use_keyed_state(self.child_id("drop"), cx, |_, _| None::<bool>);
        let drop = self
            .on_pick
            .clone()
            .filter(|_| self.interactive())
            .map(|on_pick| DropTarget {
                kind: self.kind,
                accept: self.accept,
                over: over.clone(),
                on_pick,
            });
        let state = drop.as_ref().and(*over.read(cx));
        if self.compact {
            return attach(drop, self.field(cx));
        }
        let body = match &self.value {
            Some(picked) => attach(drop, self.card(picked, cx)),
            None => attach(drop, self.zone(state, cx)),
        };
        v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(body)
            .when_some(self.target.clone(), |picker, host| {
                picker.child(
                    h_flex()
                        .gap_1p5()
                        .min_w_0()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Upload).xsmall())
                        .child(
                            div().truncate().child(
                                rust_i18n::t!("path_picker_target", name = host).to_string(),
                            ),
                        ),
                )
            })
            .into_any_element()
    }
}

/// External drop handling shared by the zone, the selection card and the field.
#[derive(Clone)]
struct DropTarget {
    kind: Kind,
    accept: &'static [&'static str],
    over: Entity<Option<bool>>,
    on_pick: PickHandler,
}

impl DropTarget {
    fn fits(&self, paths: &[PathBuf]) -> bool {
        matches!(paths, [path] if accepts(self.kind, self.accept, path, path.is_dir()))
    }

    fn set(&self, value: Option<bool>, cx: &mut App) {
        self.over.update(cx, |over, cx| {
            if *over != value {
                *over = value;
                cx.notify();
            }
        });
    }

    fn bind<E: InteractiveElement>(self, element: E) -> E {
        let moving = self.clone();
        let exited = self.clone();
        let released = self.clone();
        let styled = self.clone();
        element
            .on_drag_move(move |event: &DragMoveEvent<ExternalPaths>, window, cx| {
                // GPUI leaves keyboard hover suppression active for file drags,
                // which would hide drag-over styles and drops; the composer
                // applies the same correction.
                if window.last_input_was_keyboard() {
                    let input = event.event.clone();
                    window.defer(cx, move |window, cx| {
                        window.dispatch_event(input.to_platform_input(), cx);
                    });
                }
                let inside = event.bounds.contains(&event.event.position);
                let value = inside.then(|| moving.fits(event.drag(cx).paths()));
                moving.set(value, cx);
            })
            .on_file_drop_exit(move |_, _, cx| exited.set(None, cx))
            .on_mouse_up_out(MouseButton::Left, move |_, _, cx| released.set(None, cx))
            .drag_over::<ExternalPaths>(move |style, paths, _, cx| {
                if styled.fits(paths.paths()) {
                    style
                        .border_color(cx.theme().drag_border)
                        .bg(cx.theme().drop_target)
                } else {
                    style.border_color(cx.theme().warning)
                }
            })
            .on_drop(move |paths: &ExternalPaths, window, cx| {
                self.set(None, cx);
                if self.fits(paths.paths()) {
                    (self.on_pick)(&paths.paths()[0], window, cx);
                } else {
                    warn("path_picker_unsupported", window, cx);
                }
            })
    }
}

fn attach(drop: Option<DropTarget>, element: impl InteractiveElement + IntoElement) -> AnyElement {
    match drop {
        Some(drop) => drop.bind(element).into_any_element(),
        None => element.into_any_element(),
    }
}

fn warn(key: &'static str, window: &mut Window, cx: &mut App) {
    crate::feedback::toast(window, tr(key), Notification::error(tr(key)), cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    const ARCHIVES: &[&str] = &["zip", "skill", "tar", "tar.gz", "tgz"];

    mod accepts {
        use super::*;

        #[test]
        fn matches_archive_extensions() {
            for name in ["one.zip", "ONE.Skill", "one.tar", "one.TAR.GZ", "one.tgz"] {
                assert!(
                    accepts(Kind::File, ARCHIVES, Path::new(name), false),
                    "{name}"
                );
            }
        }

        #[test]
        fn rejects_other_files() {
            for name in ["one", "one.json", "one.zip.exe", "zip", ".zip", "one.gz"] {
                assert!(
                    !accepts(Kind::File, ARCHIVES, Path::new(name), false),
                    "{name}"
                );
            }
        }

        #[test]
        fn separates_files_and_directories() {
            let path = Path::new("skills.zip");
            assert!(!accepts(Kind::File, ARCHIVES, path, true));
            assert!(accepts(Kind::Directory, ARCHIVES, path, true));
            assert!(!accepts(Kind::Directory, &[], Path::new("notes"), false));
        }

        #[test]
        fn allows_any_file_without_filter() {
            assert!(accepts(Kind::File, &[], Path::new("id_ed25519"), false));
            assert!(!accepts(Kind::File, &[], Path::new("keys"), true));
        }
    }
}
