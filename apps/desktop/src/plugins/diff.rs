//! The pinned script catalog has no selectable unified diff leaf. Retain the core surface.
use super::host::{Host, sdk::values::decode};
use crate::content::diff;
use gpui_kit::component::{
    ActiveTheme, WindowExt,
    input::{Editor, EditorState},
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use gpui_shell::{HostError, HostModule, HostValue};
use serde::Deserialize;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Props {
    text: String,
    path: String,
    max_rows: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    text: String,
    label: String,
}

pub(super) fn module(module: HostModule, host: Arc<Host>) -> HostModule {
    let retained = Rc::new(RefCell::new(BTreeMap::<String, Entity<diff::State>>::new()));
    let sources = Rc::new(RefCell::new(BTreeMap::<String, Entity<EditorState>>::new()));
    let focus = Rc::new(RefCell::new(None::<String>));
    let request = focus.clone();
    let diff_focus = focus.clone();
    let diff_host = host.clone();
    let source_host = host.clone();
    let declarations = format!(
        "{}\nexport const DiffSurface: {{new(id:string,props:{{text:string;path:string;max_rows?:number}}):import(\"gpui-kit\").Element}};\nexport const SourceSurface: {{new(id:string,props:{{text:string;label:string}}):import(\"gpui-kit\").Element}};\nexport function focusSurface(id:string):void;",
        module.declared().unwrap_or_default()
    );
    module
        .function("focusSurface", move |args| {
            host.check()?;
            *request.borrow_mut() = Some(args.string(0)?.to_owned());
            Ok(HostValue::Null)
        })
        .component("DiffSurface", move |args, window, cx| {
            if diff_host.check().is_err() {
                return div().into_any_element();
            }
            let Ok(props) = decode(args.props()).and_then(|value| {
                serde_json::from_value::<Props>(value)
                    .map_err(|error| HostError::new(error.to_string()))
            }) else {
                return div().into_any_element();
            };
            let lines = diff::unified(&props.text);
            if diff::is_binary(&lines) {
                let selector = format!("{}-binary", args.id());
                return div()
                    .debug_selector(move || selector)
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(crate::tr("git_diff_binary"))
                    .into_any_element();
            }
            let state = retained
                .borrow_mut()
                .entry(args.id().into())
                .or_insert_with(|| {
                    cx.new(|cx| diff::State::new(&lines, diff::language(&props.path), cx))
                })
                .clone();
            state.update(cx, |state, cx| state.set_lines(&lines, cx));
            if diff_focus.borrow().as_deref() == Some(args.id()) {
                diff_focus.borrow_mut().take();
                let state = state.clone();
                let host = diff_host.clone();
                window.defer(cx, move |window, cx| {
                    if host.check().is_ok() && !window.has_active_dialog(cx) {
                        state.read(cx).focus_handle(cx).focus(window, cx);
                    }
                });
            }
            div()
                .id(SharedString::from(args.id().to_owned()))
                .debug_selector(|| args.id().to_owned())
                .size_full()
                .min_w_0()
                .min_h_0()
                .when_some(props.max_rows, |element, rows| {
                    element.h((cx.theme().mono_font_size * 1.5 + px(2.))
                        * lines.len().clamp(1, rows.max(1)) as f32)
                })
                .child(state)
                .into_any_element()
        })
        .component("SourceSurface", move |args, window, cx| {
            if source_host.check().is_err() {
                return div().into_any_element();
            }
            let Ok(props) = decode(args.props()).and_then(|value| {
                serde_json::from_value::<Source>(value)
                    .map_err(|error| HostError::new(error.to_string()))
            }) else {
                return div().into_any_element();
            };
            // Kit b79f4ce's script EditorState has no focus operation. Keep its
            // native read-only editor in this content owner for tab focus handoff.
            let state = sources
                .borrow_mut()
                .entry(args.id().into())
                .or_insert_with(|| {
                    cx.new(|cx| EditorState::new(window, cx).default_value(props.text.clone()))
                })
                .clone();
            if state.read(cx).value().as_ref() != props.text {
                state.update(cx, |state, cx| state.set_value(props.text, window, cx));
            }
            if focus.borrow().as_deref() == Some(args.id()) {
                focus.borrow_mut().take();
                let state = state.clone();
                let host = source_host.clone();
                window.defer(cx, move |window, cx| {
                    if host.check().is_ok() && !window.has_active_dialog(cx) {
                        state.update(cx, |state, cx| state.focus(window, cx));
                    }
                });
            }
            div()
                .id(SharedString::from(args.id().to_owned()))
                .debug_selector(|| args.id().to_owned())
                .size_full()
                .min_w_0()
                .min_h_0()
                .child(
                    Editor::new(&state)
                        .readonly(true)
                        .appearance(false)
                        .bordered(false)
                        .size_full()
                        .aria_label(props.label),
                )
                .into_any_element()
        })
        .declarations(declarations)
}
