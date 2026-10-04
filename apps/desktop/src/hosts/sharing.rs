//! Shared connection-scope form, composed from Kit controls.
use crate::tr;
use gpui_kit::component::form::Field;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    component::{
        checkbox::Checkbox,
        radio::{Radio, RadioGroup},
        *,
    },
    *,
};
use sailry_protocol::{Project, ProjectId, connection::Sharing};

pub(crate) struct Picker {
    scope: usize,
    selected: Vec<ProjectId>,
    pub projects: Vec<Project>,
    pub locked: bool,
}
impl Picker {
    pub fn new(sharing: Option<&Sharing>) -> Self {
        Self {
            scope: match sharing {
                None => 0,
                Some(Sharing::Global) => 1,
                Some(Sharing::Projects(_)) => 2,
            },
            selected: match sharing {
                Some(Sharing::Projects(projects)) => projects.clone(),
                _ => vec![],
            },
            projects: vec![],
            locked: false,
        }
    }
    pub fn value(&self) -> Option<Sharing> {
        match self.scope {
            1 => Some(Sharing::Global),
            2 => Some(Sharing::Projects(self.selected.clone())),
            _ => None,
        }
    }
    pub fn valid(&self) -> bool {
        self.scope != 2 || !self.selected.is_empty()
    }
}
impl Render for Picker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let choices = v_flex()
            .gap_2()
            .w_full()
            .child(
                RadioGroup::horizontal("connection-scope")
                    .gap_4()
                    .flex_wrap()
                    .disabled(self.locked)
                    .w_full()
                    .selected_index(Some(self.scope))
                    .children(
                        [
                            "connection_private",
                            "connection_global",
                            "connection_projects",
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, key)| {
                            Radio::new(index)
                                .text_sm()
                                .label(tr(key))
                                .debug_selector(move || format!("connection-scope-{index}"))
                        }),
                    )
                    .on_click(cx.listener(|this, &index, _, cx| {
                        this.scope = index;
                        cx.notify();
                    })),
            )
            .when(self.scope == 2, |body| {
                body.child(
                    v_flex()
                        .id("connection-projects")
                        .max_h_48()
                        .overflow_y_scroll()
                        .gap_2()
                        .py_1()
                        .when(self.projects.is_empty(), |body| {
                            body.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(tr("connection_no_projects")),
                            )
                        })
                        .children(self.projects.iter().map(|project| {
                            let id = project.id;
                            Checkbox::new(id.to_string())
                                .text_sm()
                                .label(project.name.clone())
                                .checked(self.selected.contains(&id))
                                .disabled(self.locked)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    if *checked {
                                        if !this.selected.contains(&id) {
                                            this.selected.push(id);
                                        }
                                    } else {
                                        this.selected.retain(|entry| *entry != id);
                                    }
                                    cx.notify();
                                }))
                        })),
                )
            });
        gpui_kit::component::form::Form::vertical()
            .child(Field::new().label(tr("connection_scope")).child(choices))
    }
}
