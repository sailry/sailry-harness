use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Index {
    Official,
    Ali,
    Tencent,
    Tsinghua,
    Custom,
}

impl Index {
    const ALL: [Self; 5] = [
        Self::Official,
        Self::Ali,
        Self::Tencent,
        Self::Tsinghua,
        Self::Custom,
    ];

    pub(super) fn url(self) -> Option<&'static str> {
        match self {
            Self::Official | Self::Custom => None,
            Self::Ali => Some("https://mirrors.aliyun.com/pypi/simple/"),
            Self::Tencent => Some("https://mirrors.tencent.com/pypi/simple/"),
            Self::Tsinghua => Some("https://pypi.tuna.tsinghua.edu.cn/simple/"),
        }
    }

    fn saved(value: Option<&str>) -> Self {
        let Some(value) = value else {
            return Self::Official;
        };
        if value.trim_end_matches('/') == "https://pypi.org/simple" {
            return Self::Official;
        }
        Self::ALL
            .into_iter()
            .find(|index| {
                index
                    .url()
                    .is_some_and(|url| url.trim_end_matches('/') == value.trim_end_matches('/'))
            })
            .unwrap_or(Self::Custom)
    }
}

impl SearchableListItem for Index {
    type Value = Self;
    fn title(&self) -> SharedString {
        match self {
            Self::Official => "pypi.org".into(),
            Self::Ali => "mirrors.aliyun.com".into(),
            Self::Tencent => "mirrors.tencent.com".into(),
            Self::Tsinghua => "pypi.tuna.tsinghua.edu.cn".into(),
            Self::Custom => crate::tr("runtime_python_custom"),
        }
    }
    fn value(&self) -> &Self::Value {
        self
    }
}

impl Panel {
    pub(super) fn index_selector(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<Vec<Index>>> {
        let source =
            cx.new(|cx| SelectState::new(Index::ALL.to_vec(), Some(IndexPath::new(0)), window, cx));
        cx.subscribe_in(&source, window, |panel: &mut Self, _, event, window, cx| {
            if !matches!(event, SelectEvent::Confirm(_)) || panel.locked() {
                return;
            }
            let Some(index) = panel.source.read(cx).selected_value().copied() else {
                return;
            };
            if index == Index::Custom {
                cx.notify();
                return;
            }
            let Some(mut settings) = panel.python.clone() else {
                return;
            };
            let value = index.url().map(str::to_owned);
            if settings.index != value {
                settings.index = value;
                panel.submit(Command::SavePythonSettings(settings), window, cx);
            }
            cx.notify();
        })
        .detach();
        source
    }

    pub(super) fn custom_index(&self, cx: &App) -> bool {
        self.source.read(cx).selected_value() == Some(&Index::Custom)
    }

    pub(super) fn accept_index(
        &mut self,
        settings: &sailry_protocol::python::Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = self.custom_index(cx)
            && self.python.as_ref().is_some_and(|saved| {
                self.index.read(cx).value().trim() != saved.index.as_deref().unwrap_or("")
                    || Index::saved(saved.index.as_deref()) != Index::Custom
            });
        if !draft {
            self.source.update(cx, |state, cx| {
                state.set_selected_value(&Index::saved(settings.index.as_deref()), window, cx)
            });
            self.index.update(cx, |state, cx| {
                state.set_value(settings.index.clone().unwrap_or_default(), window, cx)
            });
        }
    }
}
