use super::bool_method;
mod state;
use state::State;
use std::sync::Arc;

use gpui_component::diff::{Diff, DiffChangeIndicator, DiffHunkSeparator, DiffMode};
use gpui_shell::{
    ArgumentDescriptor, ArgumentSchema, ComponentArgument, ComponentDescriptor,
    ComponentMaterializer, ComponentPayload, ComponentRegistry, ConstructorDescriptor,
    MaterializeRequest, MethodDescriptor, RegistryError, StateDescriptor, anyhow,
    gpui::{self, AppContext as _, Entity, IntoElement as _, Refineable as _, Styled as _},
};

#[derive(Clone)]
enum Op {
    LineNumber(bool),
    SyntaxHighlight(bool),
    HeaderVisible(bool),
    ChangeBackground(bool),
    SoftWrap(bool),
    HunkSeparator(DiffHunkSeparator),
    ChangeIndicator(DiffChangeIndicator),
    RenderAnnotation(ComponentArgument),
    AddAnnotation(ComponentArgument),
}

fn require_leaf(children: usize) -> anyhow::Result<()> {
    anyhow::ensure!(children == 0, "Diff does not accept children");
    Ok(())
}

struct Materializer;
impl ComponentMaterializer for Materializer {
    fn materialize(&self, mut request: MaterializeRequest<'_>) -> anyhow::Result<gpui::AnyElement> {
        let argument = request
            .payload()
            .downcast_ref::<ComponentArgument>()
            .ok_or_else(|| anyhow::anyhow!("Diff received an incompatible payload"))?;
        let state = request.with_state::<Entity<State>, _>(argument, Clone::clone)?;
        let mut render_annotation = None;
        let mut add_annotation = None;
        let mut options = Vec::new();
        for op in request
            .methods()
            .filter_map(|method| method.payload().downcast_ref::<Op>())
        {
            match op {
                Op::RenderAnnotation(argument) => {
                    render_annotation = Some(request.resolve_element_callback(argument)?)
                }
                Op::AddAnnotation(argument) => {
                    add_annotation = Some(request.resolve_callback(argument)?)
                }
                _ => options.push(op.clone()),
            }
        }
        require_leaf(request.children_len())?;
        let style = request.take_style();
        request.with_window_app(|_, cx| {
            state.update(cx, |state, _| {
                state.options = options;
                state.style = style;
                state.render_annotation = render_annotation;
                state.add_annotation = add_annotation;
            });
            Ok(())
        })?;
        Ok(state.into_any_element())
    }
}

impl gpui::Render for State {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let mut diff = Diff::new(&self.diff).annotations(self.annotations.clone());
        for op in &self.options {
            diff = match op {
                Op::LineNumber(value) => diff.line_number(*value),
                Op::SyntaxHighlight(value) => diff.syntax_highlight(*value),
                Op::HeaderVisible(value) => diff.header_visible(*value),
                Op::ChangeBackground(value) => diff.change_background(*value),
                Op::SoftWrap(value) => diff.soft_wrap(*value),
                Op::HunkSeparator(value) => diff.hunk_separator(*value),
                Op::ChangeIndicator(value) => diff.change_indicator(*value),
                Op::RenderAnnotation(_) | Op::AddAnnotation(_) => diff,
            };
        }
        if let Some(callback) = self.render_annotation.clone() {
            diff = diff.render_annotation(move |annotation, window, cx| {
                let mut data = vec![
                    (
                        "id".into(),
                        gpui_shell::ComponentDataValue::String(annotation.id().to_string()),
                    ),
                    (
                        "path".into(),
                        gpui_shell::ComponentDataValue::String(annotation.path().to_string()),
                    ),
                ];
                if let Some(position) = annotation.position() {
                    data.push((
                        "side".into(),
                        gpui_shell::ComponentDataValue::String(
                            if position.side() == gpui_component::diff::DiffSide::Original {
                                "original"
                            } else {
                                "modified"
                            }
                            .into(),
                        ),
                    ));
                    data.push((
                        "line".into(),
                        gpui_shell::ComponentDataValue::Number(position.line() as f64),
                    ));
                }
                match callback.build_interactive_data_with(
                    &[gpui_shell::ComponentDataValue::Object(data)],
                    window,
                    cx,
                ) {
                    Ok(Some(element)) => element,
                    _ => gpui::div().into_any_element(),
                }
            });
        }
        if let Some(callback) = self.add_annotation.clone() {
            diff = diff.on_add_annotation(move |range, window, cx| {
                let _ = callback.invoke_data_with(&[state::range_data(range)], window, cx);
            });
        }
        diff.style().refine(&self.style);
        diff
    }
}

fn enum_method(
    name: &'static str,
    values: &'static [&'static str],
    documentation: &'static str,
    make: fn(&str) -> Option<Op>,
) -> MethodDescriptor {
    MethodDescriptor::new(
        name,
        vec![ArgumentDescriptor::new(name, ArgumentSchema::Enum(values))],
        move |args| match args {
            [ComponentArgument::Enum(value)] => make(value)
                .map(ComponentPayload::new)
                .ok_or_else(|| format!("Diff.{name} expects one of {}", values.join(", "))),
            _ => Err(format!("Diff.{name} expects one of {}", values.join(", "))),
        },
    )
    .with_documentation(documentation)
}

pub(super) fn register(registry: &mut ComponentRegistry) -> Result<(), RegistryError> {
    registry.register_state(
        StateDescriptor::new(
            "DiffState",
            "DiffState",
            vec![
                ArgumentDescriptor::new("patch", ArgumentSchema::String),
                ArgumentDescriptor::new(
                    "mode",
                    ArgumentSchema::Optional(Box::new(ArgumentSchema::Enum(&[
                        "unified", "split",
                    ]))),
                ),
            ],
            |args, _, cx| match args {
                [
                    ComponentArgument::String(patch),
                    ComponentArgument::Optional(mode),
                ] => {
                    let mode = match mode.as_deref() {
                        Some(ComponentArgument::Enum(mode)) if mode == "split" => DiffMode::Split,
                        Some(ComponentArgument::Enum(_)) | None => DiffMode::Unified,
                        Some(_) => return Err("DiffState mode expects unified or split".into()),
                    };
                    Ok(Box::new(cx.new(|cx| {
                        let mut state = State::new(mode, cx);
                        state.set_patches(vec![patch.clone()], cx);
                        state
                    })) as _)
                }
                _ => Err("DiffState expects a unified or Git patch and an optional mode".into()),
            },
        )
        .with_methods(state::methods())
        .with_documentation(
            "Retained readonly patch state parsed from unified or Git diff text, in unified or split mode.",
        ),
    )?;
    registry.register(
        ComponentDescriptor::new("Diff", Arc::new(Materializer))
            .with_constructors(vec![ConstructorDescriptor::new(
                "Diff",
                vec![ArgumentDescriptor::new(
                    "state",
                    ArgumentSchema::Entity("DiffState"),
                )],
                |args| match args {
                    [argument @ ComponentArgument::Entity { .. }] => {
                        Ok(ComponentPayload::new(argument.clone()))
                    }
                    _ => Err("Diff expects one DiffState entity".into()),
                },
            )])
            .with_methods(vec![
                MethodDescriptor::new("render_annotation", vec![ArgumentDescriptor::new("render", ArgumentSchema::Callback("(annotation: {id: string; path: string; side?: 'original' | 'modified'; line?: number}, cx: Context) => Element | null"))], |args| match args {
                    [argument @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(Op::RenderAnnotation(argument.clone()))),
                    _ => Err("Diff.render_annotation expects a renderer".into()),
                }).with_documentation("Renders plugin-owned content for each retained annotation."),
                MethodDescriptor::new("on_add_annotation", vec![ArgumentDescriptor::new("listener", ArgumentSchema::Callback("(range: {path: string; side: 'original' | 'modified'; end_side: 'original' | 'modified'; start: number; end: number}, cx: Context) => void"))], |args| match args {
                    [argument @ ComponentArgument::Callback(_)] => Ok(ComponentPayload::new(Op::AddAnnotation(argument.clone()))),
                    _ => Err("Diff.on_add_annotation expects a listener".into()),
                }).with_documentation("Reports an annotation action without writing review data."),
                bool_method("Diff", "line_number", "Shows line numbers.", Op::LineNumber),
                bool_method(
                    "Diff",
                    "syntax_highlight",
                    "Emphasizes syntax.",
                    Op::SyntaxHighlight,
                ),
                bool_method(
                    "Diff",
                    "header_visible",
                    "Shows a header above each file.",
                    Op::HeaderVisible,
                ),
                bool_method(
                    "Diff",
                    "change_background",
                    "Tints changed lines.",
                    Op::ChangeBackground,
                ),
                bool_method(
                    "Diff",
                    "soft_wrap",
                    "Wraps long lines at the column width.",
                    Op::SoftWrap,
                ),
                enum_method(
                    "hunk_separator",
                    &["metadata", "line_info", "simple"],
                    "Marks the start of each hunk.",
                    |value| {
                        Some(Op::HunkSeparator(match value {
                            "metadata" => DiffHunkSeparator::Metadata,
                            "line_info" => DiffHunkSeparator::LineInfo,
                            "simple" => DiffHunkSeparator::Simple,
                            _ => return None,
                        }))
                    },
                ),
                enum_method(
                    "change_indicator",
                    &["signs", "bars", "none"],
                    "Marks changed lines beside the code.",
                    |value| {
                        Some(Op::ChangeIndicator(match value {
                            "signs" => DiffChangeIndicator::Signs,
                            "bars" => DiffChangeIndicator::Bars,
                            "none" => DiffChangeIndicator::None,
                            _ => return None,
                        }))
                    },
                ),
            ])
            .with_documentation(
                "A readonly patch viewer for one or more files. Shell style is honored; children are rejected.",
            ),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_is_an_exact_leaf() {
        assert!(require_leaf(0).is_ok());
        assert_eq!(
            require_leaf(1).unwrap_err().to_string(),
            "Diff does not accept children"
        );
    }
}
