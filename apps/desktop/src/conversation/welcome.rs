use crate::{shell::Shell, tr};
use gpui_kit::component::{
    button::{Button, ButtonCustomVariant, ButtonVariants},
    scroll::ScrollableElement,
    *,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

type PromptHandler = std::rc::Rc<dyn Fn(SharedString, &mut Window, &mut App)>;
const PAIR_WIDTH: f32 = 320.;
mod wordmark;

impl Shell {
    pub(super) fn welcome(&self, cx: &mut Context<Self>) -> AnyElement {
        let key = (self.host, self.session);
        let owner = cx.entity().downgrade();
        content(
            std::rc::Rc::new(move |prompt, window, cx| {
                let _ = owner.update(cx, |shell, cx| {
                    if let Some(conversation) = shell.conversations.get(&key) {
                        conversation.input.update(cx, |input, cx| {
                            input.set_value(prompt, window, cx);
                            input.focus(window, cx);
                        });
                    }
                });
            }),
            self.composer(cx),
            cx,
        )
    }
}

pub(super) fn content(on_prompt: PromptHandler, composer: AnyElement, _cx: &App) -> AnyElement {
    container_query(move |size, window, cx| {
        let light = window.use_keyed_state("welcome-light", cx, |_, _| wordmark::Light::default());
        layout(on_prompt, composer, size.width, window, light, cx)
    })
    .into_any_element()
}

fn layout(
    on_prompt: PromptHandler,
    composer: AnyElement,
    width: Pixels,
    window: &mut Window,
    light: Entity<wordmark::Light>,
    cx: &App,
) -> AnyElement {
    let rem = window.rem_size();
    let theme = cx.theme();
    let compact = width < px(PAIR_WIDTH);
    let logo_size = if width < px(180.) {
        px(20.)
    } else if width < px(240.) {
        px(24.)
    } else if compact {
        px(32.)
    } else {
        px(72.)
    };
    // Keep room for optional theme branding in the same wordmark row.
    let padding = rem * if compact { 0.5 } else { 1.5 };
    let available = (width - padding * 2. - rem * 0.75).max(px(0.));
    let logo_size = logo_size.min(px(f32::from(available / (wordmark::RATIO + 1.)).floor()));
    let hero = v_flex()
        .debug_selector(|| "conversation-welcome".into())
        .w_full()
        .px_6()
        .when(compact, |hero| hero.px_2())
        .gap_4()
        .items_center()
        .children(crate::theme::banner("hero.new_session", cx))
        .child(
            h_flex()
                .debug_selector(|| "welcome-wordmark".into())
                .gap_3()
                .py_1()
                .text_color(theme.foreground)
                .children(crate::theme::brand(logo_size, cx).map(|brand| {
                    div()
                        .debug_selector(|| "welcome-brand".into())
                        .flex_shrink_0()
                        .child(brand)
                }))
                .child(wordmark::render(logo_size, light.read(cx).pointer, cx)),
        )
        .child(
            div()
                .w_full()
                .text_center()
                .text_lg()
                .whitespace_normal()
                .child(tr("welcome_title")),
        );
    let [explore, build, review, plan] = [
        (
            "welcome_explore",
            "welcome_explore_detail",
            "welcome_explore_prompt",
            IconName::Folder,
            theme.blue,
        ),
        (
            "welcome_build",
            "welcome_build_detail",
            "welcome_build_prompt",
            IconName::SquareTerminal,
            theme.green,
        ),
        (
            "welcome_review",
            "welcome_review_detail",
            "welcome_review_prompt",
            IconName::Eye,
            theme.yellow,
        ),
        (
            "welcome_plan",
            "welcome_plan_detail",
            "welcome_plan_prompt",
            IconName::LayoutDashboard,
            theme.magenta,
        ),
    ]
    .map(|(title, detail, prompt, icon, color)| {
        let on_prompt = on_prompt.clone();
        Button::new(title)
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(color)
                    .hover(color.opacity(0.2))
                    .active(color.opacity(0.26)),
            )
            .h_auto()
            .flex_1()
            .min_w_0()
            .p_4()
            .debug_selector(move || title.into())
            .accessibility_label(tr(title))
            .border_0()
            .rounded(theme.radius_2xl())
            .child(
                v_flex()
                    .w_full()
                    .min_w_0()
                    .gap_4()
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .child(Icon::new(icon).size_6().text_color(color))
                            .child(
                                Icon::new(IconName::ArrowRight)
                                    .size_4()
                                    .text_color(color.opacity(0.5)),
                            ),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap_1()
                            .text_left()
                            .child(div().text_sm().child(tr(title)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .whitespace_normal()
                                    .child(tr(detail)),
                            ),
                    ),
            )
            .on_click(move |_, window, cx| on_prompt(tr(prompt), window, cx))
    });
    let suggestions = h_flex()
        .flex_wrap()
        .items_stretch()
        .gap_3()
        .w_full()
        .px_6()
        .when(compact, |cards| cards.px_2())
        .children([[explore, build], [review, plan]].map(|cards| {
            h_flex()
                .when(compact, |row| row.flex_col())
                .flex_1()
                .flex_basis(px(PAIR_WIDTH))
                .min_w_0()
                .items_stretch()
                .gap_3()
                .children(cards)
        }));
    v_flex()
        .id("welcome-scroll")
        .size_full()
        .min_h_0()
        .on_mouse_move(
            window.listener_for(&light, |light, event: &MouseMoveEvent, _, cx| {
                if light.set(event.position.x) {
                    cx.notify();
                }
            }),
        )
        .overflow_y_scrollbar()
        .child(
            v_flex()
                .w_full()
                .min_h(relative(1.))
                .flex_shrink_0()
                .max_w(px(super::CONTENT_WIDTH))
                .mx_auto()
                .py_8()
                .gap_3()
                .justify_center()
                .child(hero)
                .child(composer)
                .child(suggestions),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
