//! Native local-time editing, without scheduling or reminder business rules.
use chrono::{Local, TimeZone};
use gpui_kit::{
    component::{
        Disableable as _,
        calendar::Date,
        date_picker::{DatePicker, DatePickerState},
        time_field::{TimeField, TimeFieldState, TimePrecision},
    },
    *,
};
use gpui_shell::{HostError, HostModule, HostValue};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

struct Draft {
    original: i64,
    label: String,
    state: Option<(Entity<DatePickerState>, Entity<TimeFieldState>)>,
}

impl Draft {
    fn read(&self, cx: &App) -> Result<i64, HostError> {
        let Some((date, time)) = &self.state else {
            return Ok(self.original);
        };
        let Date::Single(Some(date)) = date.read(cx).date() else {
            return Err(invalid());
        };
        let time = time.read(cx).time();
        // Gaps and ambiguous local times require an explicit new selection.
        let at = Local
            .from_local_datetime(&date.and_time(time))
            .single()
            .ok_or_else(invalid)?
            .timestamp_millis();
        Ok(if at == self.original.div_euclid(1000) * 1000 {
            self.original
        } else {
            at
        })
    }

    fn render(&mut self, disabled: bool, window: &mut Window, cx: &mut App) -> AnyElement {
        let (date, time) = self.state.get_or_insert_with(|| {
            let at = chrono::DateTime::from_timestamp_millis(self.original)
                .expect("validated date")
                .with_timezone(&Local);
            let date = cx.new(|cx| {
                let mut state = DatePickerState::new(window, cx).date_format("%Y-%m-%d");
                state.set_date(at.date_naive(), window, cx);
                state
            });
            let time = cx.new(|cx| {
                let mut state = TimeFieldState::new(window, cx).precision(TimePrecision::Second);
                state.set_time(at.time(), window, cx);
                state
            });
            (date, time)
        });
        component::h_flex()
            .gap_2()
            .w_full()
            .min_w_0()
            .child(
                div()
                    .debug_selector(|| "date-time-date".into())
                    .flex_1()
                    .min_w_0()
                    .child(
                        DatePicker::new(date)
                            .placeholder(crate::tr("form_date_hint"))
                            .disabled(disabled),
                    ),
            )
            .child(
                div()
                    .id(("date-time-time", time.entity_id()))
                    .debug_selector(|| "date-time-time".into())
                    .role(Role::Group)
                    .aria_label(self.label.clone())
                    .w_32()
                    .flex_shrink_0()
                    .child(TimeField::new(time).min_w_0().disabled(disabled)),
            )
            .into_any_element()
    }
}

fn invalid() -> HostError {
    HostError::new("invalid local date or time")
}

#[derive(Default)]
struct Fields {
    sequence: u64,
    drafts: BTreeMap<String, Draft>,
}

pub(super) fn extend(module: HostModule) -> HostModule {
    let fields = Rc::new(RefCell::new(Fields::default()));
    let create = fields.clone();
    let read = fields.clone();
    let release = fields.clone();
    module
        .function("createDateTime", move |args| {
            let value = args.number(0)?;
            if !value.is_finite() || value.fract() != 0. || value.abs() > 9_007_199_254_740_991. {
                return Err(invalid());
            }
            let original = value as i64;
            chrono::DateTime::from_timestamp_millis(original).ok_or_else(invalid)?;
            let label = args.string(1)?.to_owned();
            let mut fields = create.borrow_mut();
            if fields.drafts.len() >= super::MAX_FIELDS {
                return Err(HostError::new("plugin field capacity exhausted"));
            }
            fields.sequence += 1;
            let id = format!("date-{}", fields.sequence);
            fields.drafts.insert(
                id.clone(),
                Draft {
                    original,
                    label,
                    state: None,
                },
            );
            Ok(HostValue::from(id))
        })
        .function("readDateTime", move |args| {
            let fields = read.borrow();
            let draft = fields
                .drafts
                .get(args.string(0)?)
                .ok_or_else(|| HostError::new("plugin field is unavailable"))?;
            gpui_shell::with_current_app(|cx| {
                draft.read(cx).map(|value| HostValue::Number(value as f64))
            })
            .ok_or_else(|| HostError::new("plugin field requires an active view"))?
        })
        .function("releaseDateTime", move |args| {
            release.borrow_mut().drafts.remove(args.string(0)?);
            Ok(HostValue::Null)
        })
        .component("DateTimeField", move |args, window, cx| {
            let mut fields = fields.borrow_mut();
            let Some(draft) = fields.drafts.get_mut(args.id()) else {
                return div().into_any_element();
            };
            let disabled = args.props().get("disabled").and_then(HostValue::as_bool) == Some(true);
            let view = div().w_full().child(draft.render(disabled, window, cx));
            #[cfg(test)]
            let view = view.debug_selector(|| args.id().to_owned());
            view.into_any_element()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    struct Field {
        width: Pixels,
        draft: Draft,
    }

    impl Render for Field {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .debug_selector(|| "date-time-field".into())
                .w(self.width)
                .child(self.draft.render(false, window, cx))
        }
    }

    #[gpui_kit::test]
    fn keeps_both_controls_inside_the_row(cx: &mut TestAppContext) {
        crate::plugins::tests::init(cx);
        for width in [240., 440.] {
            let mut owner = None;
            let (_, visual) = cx.add_window_view(|window, cx| {
                let field = cx.new(|_| Field {
                    width: px(width),
                    draft: Draft {
                        original: 1790656200123,
                        label: "Time".into(),
                        state: None,
                    },
                });
                owner = Some(field.clone());
                component::Root::new(field, window, cx)
            });
            let field = owner.unwrap();
            visual.run_until_parked();
            visual.update(|window, cx| window.draw(cx).clear(cx));
            let row = visual.debug_bounds("date-time-field").unwrap();
            let date = visual.debug_bounds("date-time-date").unwrap();
            let time = visual.debug_bounds("date-time-time").unwrap();
            assert_eq!(row.size.width, px(width));
            assert_eq!(date.top(), time.top());
            assert!(date.left() >= row.left() && time.right() <= row.right());
            assert!(date.right() < time.left());
            assert_eq!(time.size.width, px(128.));
            visual.update(|window, cx| {
                let time = field.read(cx).draft.state.as_ref().unwrap().1.clone();
                time.update(cx, |state, cx| state.focus(window, cx));
            });
            visual.simulate_keystrokes("1 3 1 4 1 5");
            field.read_with(visual, |field, cx| {
                assert_eq!(
                    field.draft.state.as_ref().unwrap().1.read(cx).time(),
                    chrono::NaiveTime::from_hms_opt(13, 14, 15).unwrap()
                );
                let value = field.draft.read(cx).unwrap();
                assert_eq!(
                    chrono::DateTime::from_timestamp_millis(value)
                        .unwrap()
                        .with_timezone(&Local)
                        .format("%H:%M:%S")
                        .to_string(),
                    "13:14:15"
                );
            });
            crate::conversation::live::tests::fixture::tap(visual, "date-time-date");
            visual.simulate_keystrokes("escape");
            visual.update(|window, _| window.remove_window());
        }
    }

    #[gpui_kit::test]
    fn preserves_subseconds_and_requires_a_date(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (_, visual) = cx.add_window_view(|_, _| Empty);
        visual.update(|window, cx| {
            let original = 1790656200123;
            let mut draft = Draft {
                original,
                label: "Time".into(),
                state: None,
            };
            assert_eq!(draft.read(cx).unwrap(), original);
            let _ = draft.render(false, window, cx);
            assert_eq!(draft.read(cx).unwrap(), original);
            let (date, time) = draft.state.as_ref().unwrap();
            let original_time = time.read(cx).time();
            time.update(cx, |state, cx| state.set_time(original_time, window, cx));
            assert_eq!(draft.read(cx).unwrap(), original);
            date.update(cx, |state, cx| {
                state.set_date(Date::Single(None), window, cx)
            });
            assert!(draft.read(cx).is_err());
        });
    }
}
