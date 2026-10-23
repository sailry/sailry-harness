use super::*;
use crate::feedback::Recovery as _;
use gpui_kit::component::{
    label::Label,
    list::{ListDelegate, ListItem},
};
use sailry_protocol::conversation::search::{MAX_QUERY_BYTES, Query};

pub(super) struct Results {
    client: Arc<Client>,
    runtime: Arc<tokio::runtime::Runtime>,
    pub session: Option<SessionId>,
    pub revision: Option<u64>,
    pub query: String,
    pub query_generation: u64,
    pub matches: Vec<Match>,
    pub before: Option<u64>,
    pub loading: bool,
    pub error: Option<&'static str>,
    pub more: Option<Task<()>>,
    stop: CancellationToken,
}

impl Drop for Results {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl Results {
    pub(super) fn new(binding: Binding, session: Option<SessionId>) -> Self {
        Self {
            client: binding.client,
            runtime: binding.runtime,
            session,
            revision: None,
            query: String::new(),
            query_generation: 0,
            matches: Vec::new(),
            before: None,
            loading: false,
            error: None,
            more: None,
            stop: CancellationToken::new(),
        }
    }

    pub(super) fn cancel(&mut self) {
        self.stop.cancel();
        self.loading = false;
        self.more = None;
    }

    pub(super) fn request(
        &mut self,
        before: Option<u64>,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.cancel();
        self.error = None;
        let Some(session) = self.session else {
            return Task::ready(());
        };
        if self.query.is_empty() {
            return Task::ready(());
        }
        if self.query.len() > MAX_QUERY_BYTES {
            self.error = Some("chat_search_limit");
            cx.notify();
            return Task::ready(());
        }
        self.loading = true;
        let stop = CancellationToken::new();
        self.stop = stop.clone();
        let cancelled = stop.clone();
        let client = self.client.clone();
        let query = Query {
            text: self.query.clone(),
            case_sensitive: false,
            before,
            limit: 20,
        };
        let job = self.runtime.spawn(async move {
            tokio::select! {
                _ = cancelled.cancelled() => None,
                result = async {
                    if before.is_none() { tokio::time::sleep(std::time::Duration::from_millis(180)).await; }
                    client.search_conversation(session, query).await
                } => Some(result),
            }
        });
        cx.notify();
        cx.spawn_in(window, async move |list, cx| {
            let result = job.await;
            if stop.is_cancelled() {
                return;
            }
            let _ = list.update_in(cx, |list, window, cx| {
                if stop.is_cancelled() {
                    return;
                }
                let results = list.delegate_mut();
                results.loading = false;
                match result {
                    Ok(Some(Ok(page))) if Some(page.revision) == results.revision => {
                        if before.is_none() {
                            results.matches.clear();
                        }
                        results.matches.extend(page.matches);
                        results.before = page.next_before;
                        if before.is_none() {
                            let selected =
                                (!results.matches.is_empty()).then_some(IndexPath::default());
                            list.set_selected_index(selected, window, cx);
                        }
                    }
                    _ => results.error = Some("chat_search_failed"),
                }
                cx.notify();
            });
        })
    }
}

impl ListDelegate for Results {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.matches.len()
    }

    fn set_selected_index(
        &mut self,
        _: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
    }

    fn perform_search(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Task<()> {
        self.query = query.into();
        self.query_generation += 1;
        self.matches.clear();
        self.before = None;
        cx.notify();
        self.request(None, window, cx)
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let found = self.matches.get(index.row)?;
        let start = found.snippet[..found.highlight.start]
            .char_indices()
            .rev()
            .nth(12)
            .map_or(0, |(index, _)| index);
        let snippet = if start > 0 {
            format!("…{}", &found.snippet[start..])
        } else {
            found.snippet.clone()
        };
        Some(
            ListItem::new(index.row)
                .h_20()
                .min_w_0()
                .rounded(cx.theme().radius)
                // Kit's virtual List has no gap option; row margins are measured.
                .mb(px(1.))
                .px_2()
                .py_2()
                .accessibility_label(found.snippet.clone())
                .child(
                    v_flex()
                        .w_full()
                        .min_w_0()
                        .debug_selector(move || format!("live-search-result-{}", index.row))
                        .child(
                            Label::new(snippet)
                                .highlights(found.snippet[found.highlight.clone()].to_owned())
                                .w_full()
                                .min_w_0()
                                .text_sm()
                                .line_height(relative(1.25))
                                .whitespace_normal()
                                .line_clamp(3),
                        ),
                ),
        )
    }

    fn has_more(&self, _: &App) -> bool {
        self.before.is_some() && !self.loading && self.error.is_none()
    }

    fn load_more_threshold(&self) -> usize {
        3
    }

    fn load_more(&mut self, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        if self.has_more(cx) {
            self.more = Some(self.request(self.before, window, cx));
        }
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
    }
}

impl Results {
    pub(super) fn observe_errors(
        search: WeakEntity<Search>,
        window: &Window,
        cx: &mut Context<ListState<Self>>,
    ) {
        crate::feedback::observe_with(
            window,
            cx,
            |list: &ListState<Self>, _| list.delegate().error.into_iter().collect(),
            move |_, key, _| {
                let notice = gpui_kit::component::notification::Notification::error(tr(key));
                if key == "chat_search_limit" {
                    return notice.into();
                }
                let owner = search.clone();
                notice.scoped_action(move |_, _, cx| {
                    let owner = owner.clone();
                    crate::feedback::retry("live-search-retry")
                        .debug_selector(|| "live-search-retry".into())
                        .on_click(cx.listener(move |toast, _, window, cx| {
                            toast.dismiss(window, cx);
                            let owner = owner.clone();
                            window.defer(cx, move |window, cx| {
                                _ = owner.update(cx, |search, cx| search.retry(window, cx));
                            });
                        }))
                })
            },
        );
    }
}
