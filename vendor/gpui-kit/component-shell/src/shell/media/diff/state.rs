//! Retained parsing and review operations missing from the upstream script catalog.
use gpui_component::WindowExt as _;
use gpui_component::diff::{
    DiffAnnotation, DiffFile, DiffLinePosition, DiffLineRange, DiffMode, DiffSide, DiffState,
};
use gpui_shell::{
    ComponentDataValue as Data, StateMethodDescriptor,
    anyhow::{self, bail},
    gpui::{self, AppContext as _, Context, Entity, Focusable as _, Task},
};

pub(super) struct State {
    pub diff: Entity<DiffState>,
    pub annotations: Vec<DiffAnnotation>,
    pub options: Vec<super::Op>,
    pub style: gpui::StyleRefinement,
    pub render_annotation: Option<gpui_shell::ComponentElementCallback>,
    pub add_annotation: Option<gpui_shell::ComponentCallback>,
    patches: Option<Vec<String>>,
    generation: u64,
    task: Option<Task<()>>,
    error: Option<String>,
}
impl State {
    pub fn new(mode: DiffMode, cx: &mut Context<Self>) -> Self {
        let diff = cx.new(|cx| {
            DiffState::new([], cx)
                .with_mode(mode)
                .with_context_lines(None)
        });
        cx.observe(&diff, |_, _, cx| cx.notify()).detach();
        Self {
            diff,
            annotations: Vec::new(),
            options: Vec::new(),
            style: gpui::StyleRefinement::default(),
            render_annotation: None,
            add_annotation: None,
            patches: None,
            generation: 0,
            task: None,
            error: None,
        }
    }
    pub fn set_patches(&mut self, patches: Vec<String>, cx: &mut Context<Self>) {
        if self.patches.as_ref() == Some(&patches) {
            return;
        }
        self.task = None;
        self.patches = Some(patches.clone());
        self.generation += 1;
        let generation = self.generation;
        self.error = None;
        let parse = cx.background_executor().spawn(async move {
            let mut files = Vec::new();
            for patch in patches {
                if !patch.is_empty() {
                    files.extend(DiffFile::parse(&patch).map_err(|error| error.to_string())?);
                }
            }
            Ok::<_, String>(files)
        });
        self.task = Some(cx.spawn(async move |owner, cx| {
            let result = parse.await;
            let _ = owner.update(cx, |owner, cx| {
                if owner.generation != generation {
                    return;
                }
                owner.task = None;
                match result {
                    Ok(files) => owner
                        .diff
                        .update(cx, |state, cx| state.set_files(files, cx)),
                    Err(error) => {
                        owner.error = Some(error);
                        owner.diff.update(cx, |state, cx| state.set_files([], cx));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}

fn text(value: &Data) -> anyhow::Result<&str> {
    match value {
        Data::String(value) => Ok(value),
        _ => bail!("expected text"),
    }
}
fn line(value: &Data) -> anyhow::Result<usize> {
    match value {
        Data::Number(value)
            if value.is_finite()
                && *value >= 1.
                && value.fract() == 0.
                && *value <= usize::MAX as f64 =>
        {
            Ok(*value as usize)
        }
        _ => bail!("expected a positive source line"),
    }
}
fn side(value: &Data) -> anyhow::Result<DiffSide> {
    match text(value)? {
        "original" => Ok(DiffSide::Original),
        "modified" => Ok(DiffSide::Modified),
        _ => bail!("expected original or modified"),
    }
}
pub(super) fn position(
    path: &Data,
    value: &Data,
    number: &Data,
) -> anyhow::Result<DiffLinePosition> {
    Ok(DiffLinePosition::new(
        text(path)?.to_owned(),
        side(value)?,
        line(number)?,
    ))
}
pub(super) fn range_data(value: &DiffLineRange) -> Data {
    Data::Object(vec![
        ("path".into(), Data::String(value.path().to_string())),
        (
            "side".into(),
            Data::String(
                if value.side() == DiffSide::Original {
                    "original"
                } else {
                    "modified"
                }
                .into(),
            ),
        ),
        (
            "end_side".into(),
            Data::String(
                if value.end_side() == DiffSide::Original {
                    "original"
                } else {
                    "modified"
                }
                .into(),
            ),
        ),
        ("start".into(), Data::Number(value.start() as f64)),
        ("end".into(), Data::Number(value.end() as f64)),
    ])
}
fn annotations(value: &Data) -> anyhow::Result<Vec<DiffAnnotation>> {
    let Data::Array(values) = value else {
        bail!("expected annotation array");
    };
    values
        .iter()
        .map(|value| {
            let Data::Object(fields) = value else {
                bail!("expected annotation object");
            };
            let field = |name: &str| {
                fields
                    .iter()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value)
                    .ok_or_else(|| anyhow::anyhow!("missing annotation {name}"))
            };
            let id = text(field("id")?)?.to_owned();
            let path = field("path")?;
            if id.is_empty() || text(path)?.is_empty() {
                bail!("annotation identity must be nonempty");
            }
            if fields.iter().any(|(key, _)| key == "line") {
                Ok(DiffAnnotation::line(
                    id,
                    position(path, field("side")?, field("line")?)?,
                ))
            } else {
                Ok(DiffAnnotation::file(id, text(path)?.to_owned()))
            }
        })
        .collect()
}

pub(super) fn methods() -> Vec<StateMethodDescriptor> {
    const METHODS: &[(&str, &str, bool)] = &[
        ("set_patch", "(patch: string): void", false),
        ("set_files", "(patches: string[]): void", false),
        ("focus", "(): void", false),
        ("set_mode", "(mode: 'unified' | 'split'): void", false),
        ("scroll_to_file", "(path: string): void", false),
        (
            "scroll_to_line",
            "(path: string, side: 'original' | 'modified', line: number): void",
            false,
        ),
        ("next_change", "(): void", false),
        ("previous_change", "(): void", false),
        (
            "set_annotations",
            "(annotations: {id: string; path: string; side?: 'original' | 'modified'; line?: number}[]): void",
            false,
        ),
        (
            "selected_lines",
            "(): {path: string; side: 'original' | 'modified'; end_side: 'original' | 'modified'; start: number; end: number} | null",
            true,
        ),
        ("pending", "(): boolean", true),
        ("error", "(): string | null", true),
    ];
    METHODS
        .iter()
        .map(|&(name, signature, readonly)| {
            StateMethodDescriptor::new::<Entity<State>>(
                name,
                signature,
                move |entity, args, window, cx| {
                    let diff = entity.read(cx).diff.clone();
                    match (name, args) {
                        ("set_patch", [patch]) => {
                            let patch = text(patch)?.to_owned();
                            entity.update(cx, |state, cx| state.set_patches(vec![patch], cx));
                        }
                        ("set_files", [Data::Array(patches)]) => {
                            let patches = patches
                                .iter()
                                .map(|value| text(value).map(str::to_owned))
                                .collect::<anyhow::Result<_>>()?;
                            entity.update(cx, |state, cx| state.set_patches(patches, cx));
                        }
                        ("focus", []) => {
                            if !window.has_active_dialog(cx) {
                                diff.read(cx).focus_handle(cx).focus(window, cx);
                            }
                        }
                        ("set_mode", [mode]) => {
                            let mode = match text(mode)? {
                                "unified" => DiffMode::Unified,
                                "split" => DiffMode::Split,
                                _ => bail!("expected unified or split"),
                            };
                            diff.update(cx, |state, cx| state.set_mode(mode, cx));
                        }
                        ("scroll_to_file", [path]) => {
                            let path = text(path)?;
                            diff.update(cx, |state, cx| state.scroll_to_file(path, cx));
                        }
                        ("scroll_to_line", [path, side, number]) => {
                            let position = position(path, side, number)?;
                            diff.update(cx, |state, cx| state.scroll_to_line(position, cx));
                        }
                        ("next_change", []) => diff.update(cx, |state, cx| state.next_change(cx)),
                        ("previous_change", []) => {
                            diff.update(cx, |state, cx| state.previous_change(cx))
                        }
                        ("set_annotations", [value]) => {
                            let annotations = annotations(value)?;
                            entity.update(cx, |state, cx| {
                                state.annotations = annotations;
                                cx.notify();
                            });
                        }
                        ("selected_lines", []) => {
                            return Ok(diff
                                .read(cx)
                                .selected_lines()
                                .as_ref()
                                .map(range_data)
                                .unwrap_or(Data::Null));
                        }
                        ("pending", []) => {
                            return Ok(Data::Boolean(entity.read(cx).task.is_some()));
                        }
                        ("error", []) => {
                            return Ok(entity
                                .read(cx)
                                .error
                                .as_ref()
                                .map(|value| Data::String(value.clone()))
                                .unwrap_or(Data::Null));
                        }
                        _ => bail!("invalid DiffState.{name} arguments"),
                    }
                    Ok(Data::Null)
                },
            )
            .with_readonly(readonly)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_cross_side_range() {
        let range = DiffLineRange::new("review.txt", DiffSide::Original, 2, 5)
            .with_end_side(DiffSide::Modified);
        let Data::Object(fields) = range_data(&range) else {
            panic!("range object expected")
        };
        let field = |name: &str| {
            fields
                .iter()
                .find(|(key, _)| key == name)
                .unwrap()
                .1
                .clone()
        };
        assert_eq!(field("side"), Data::String("original".into()));
        assert_eq!(field("end_side"), Data::String("modified".into()));
        assert_eq!(field("start"), Data::Number(2.));
        assert_eq!(field("end"), Data::Number(5.));
    }
}
