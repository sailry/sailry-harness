//! The global command surface only navigates to existing resources and pages.
use super::*;
use gpui_kit::component::{input::Backspace, list::ListState};
mod catalog;
mod list;
use self::list::Rows;
use catalog::{Folder, Scope, Target};

struct Palette {
    owner: WeakEntity<Shell>,
    list: Entity<ListState<Rows>>,
    scope: Scope,
    folder: Folder,
    dirty: bool,
    _observers: Vec<Subscription>,
}

impl Palette {
    fn new(
        owner: Entity<Shell>,
        scope: Scope,
        groups: Vec<catalog::Group>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let observer = cx.observe(&owner, |palette, _, cx| {
            palette.dirty = true;
            cx.notify();
        });
        let rows = Rows::new(cx.entity().downgrade(), groups);
        let list = cx.new(|cx| ListState::new(rows, window, cx).searchable(true));
        list.update(cx, |list, cx| {
            list.set_selected_index(Some(IndexPath::default()), window, cx)
        });
        let list_observer = cx.observe(&list, |_, _, cx| cx.notify());
        Self {
            owner: owner.downgrade(),
            list,
            scope,
            folder: Folder::Root,
            dirty: false,
            _observers: vec![observer, list_observer],
        }
    }

    fn enter(&mut self, folder: Folder, window: &mut Window, cx: &mut Context<Self>) {
        self.folder = folder;
        self.dirty = true;
        self.list.update(cx, |list, cx| {
            list.set_query("", window, cx);
            list.set_selected_index(Some(IndexPath::default()), window, cx);
            list.focus(window, cx);
        });
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.folder != Folder::Root {
            self.enter(Folder::Root, window, cx);
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        let has_query = window
            .focused_input(cx)
            .is_some_and(|input| !input.value(cx).is_empty());
        if has_query || self.folder == Folder::Root {
            cx.propagate();
            return;
        }
        self.back(window, cx);
        cx.stop_propagation();
    }

    fn click_back(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.back(window, cx);
    }

    fn confirm(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        if let Target::Folder(folder) = target {
            self.enter(folder, window, cx);
            return;
        }
        let scope = self.scope;
        let folder = self.folder;
        let _ = self.owner.update(cx, |shell, cx| {
            if catalog::available(shell, scope, folder, &target, cx) {
                window.close_dialog(cx);
                catalog::open(shell, scope, folder, target, window, cx);
            }
        });
    }
}

impl Render for Palette {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dirty {
            self.dirty = false;
            let groups = self
                .owner
                .upgrade()
                .map(|owner| catalog::entries(owner.read(cx), self.scope, self.folder, cx))
                .unwrap_or_default();
            self.list.update(cx, |list, cx| {
                list.delegate_mut().replace(groups);
                let selected = list.delegate().selected;
                list.set_selected_index(selected, window, cx);
                cx.notify();
            });
        }
        let rows = self.list.read(cx).delegate();
        let focus = self.list.focus_handle(cx);
        let show_back = self.folder != Folder::Root;
        let height = crate::command_picker::height(rows.count(), rows.headings(), window)
            + if show_back {
                px(crate::command_picker::ROW_HEIGHT)
            } else {
                px(0.)
            };
        v_flex()
            .h(height)
            .min_h_0()
            .track_focus(&focus)
            .capture_action(cx.listener(Self::backspace))
            .when(show_back, |palette| {
                palette.child(
                    h_flex()
                        .h(px(crate::command_picker::ROW_HEIGHT))
                        .flex_shrink_0()
                        .px_2()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(
                            div().debug_selector(|| "command-back".into()).child(
                                Button::new("command-back")
                                    .text()
                                    .compact()
                                    .icon(IconName::ArrowLeft)
                                    .label(tr("reference_back"))
                                    .on_click(cx.listener(Self::click_back)),
                            ),
                        ),
                )
            })
            .child(div().flex_1().min_h_0().child(crate::command_picker::list(
                &self.list,
                tr("search"),
                window,
            )))
    }
}

impl Shell {
    pub(crate) fn search(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            return;
        }
        let owner = cx.entity();
        let scope = Scope::capture(self);
        let groups = catalog::entries(self, scope, Folder::Root, cx);
        let palette = cx.new(|cx| Palette::new(owner, scope, groups, window, cx));
        let focus = palette.read(cx).list.clone();
        window.open_dialog(cx, move |dialog, window, cx| {
            crate::command_picker::frame(dialog, "global-command", palette.clone(), window, cx)
                .on_ok(|_, _, _| false)
        });
        focus.update(cx, |list, cx| list.focus(window, cx));
    }
}
