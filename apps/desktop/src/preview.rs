use gpui_kit::component::IconName;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Conversation,
    Activity,
    Files,
    Git,
    Terminal,
    Settings,
    Plugins,
    Host,
    Project,
    Plugin,
}

impl Page {
    pub const ALL: [Self; 9] = [
        Self::Conversation,
        Self::Activity,
        Self::Files,
        Self::Git,
        Self::Terminal,
        Self::Settings,
        Self::Plugins,
        Self::Host,
        Self::Project,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Activity => "activity",
            Self::Files => "files",
            Self::Git => "git",
            Self::Terminal => "terminal",
            Self::Settings => "settings",
            Self::Host => "workspace_host",
            Self::Project => "workspace_project",
            Self::Plugin | Self::Plugins => "settings_plugins",
        }
    }

    pub fn label(self) -> gpui_kit::SharedString {
        crate::tr(if self == Self::Conversation {
            "shell_sessions"
        } else {
            self.key()
        })
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::Conversation => IconName::Plus,
            Self::Activity => IconName::LayoutDashboard,
            Self::Files => IconName::Folder,
            Self::Git => IconName::Network,
            Self::Terminal => IconName::SquareTerminal,
            Self::Settings => IconName::Settings,
            Self::Host => IconName::Cpu,
            Self::Project => IconName::Folder,
            Self::Plugin | Self::Plugins => IconName::GalleryVerticalEnd,
        }
    }

    pub fn panel_index(self) -> usize {
        match self {
            Self::Files => 1,
            Self::Git => 2,
            Self::Host => 3,
            _ => 0,
        }
    }

    pub fn has_panel(self) -> bool {
        matches!(
            self,
            Self::Conversation | Self::Files | Self::Git | Self::Host
        )
    }
}

pub const FILES: [&str; 2] = ["README.md", "docs/example.md"];
pub const FILE_DIFF_KEYS: [&str; 2] = ["diff_intro", "diff_design"];
pub const NAV_WIDTH: f32 = 240.;
pub(crate) const RAIL_WIDTH: f32 = 56.;
pub const DETAIL_WIDTH: f32 = 320.;
pub(crate) const ASSISTANT_WIDTH: f32 = 400.;
pub const MAIN_MIN: f32 = 480.;
pub(crate) const CONTENT_WIDTH: f32 = 800.;
pub const HEADER_HEIGHT: f32 = 48.;

#[derive(Clone, Debug)]
pub struct Layout {
    pub sidebar_open: bool,
    pub sidebar_width: f32,
    pub panel_open: [bool; 6],
    pub panel_width: [f32; 6],
    pub conversation_panel_resized: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            sidebar_open: true,
            sidebar_width: NAV_WIDTH,
            panel_open: [false, true, true, false, true, true],
            conversation_panel_resized: false,
            panel_width: [
                560.,
                ASSISTANT_WIDTH,
                ASSISTANT_WIDTH,
                DETAIL_WIDTH,
                ASSISTANT_WIDTH,
                ASSISTANT_WIDTH,
            ],
        }
    }
}

impl Layout {
    pub fn main_min(&self, page: Page) -> f32 {
        if page == Page::Conversation {
            crate::conversation::MIN_WIDTH
        } else {
            MAIN_MIN
        }
    }

    pub fn panel_size(&self, page: Page, width: f32) -> f32 {
        if page != Page::Conversation {
            return self.panel_width[page.panel_index()];
        }
        let available = width
            - RAIL_WIDTH
            - if self.sidebar_open {
                self.sidebar_width
            } else {
                0.
            };
        let preferred = if self.conversation_panel_resized {
            self.panel_width[0]
        } else {
            available * 0.5
        };
        preferred
            .min(available - self.main_min(page))
            .max(crate::resources::MIN_WIDTH)
    }

    #[cfg(test)]
    pub fn panel_visible(&self, page: Page, width: f32) -> bool {
        self.panel_fits(page, width, self.panel_size(page, width))
    }

    #[cfg(test)]
    pub fn panel_fits(&self, page: Page, width: f32, panel_width: f32) -> bool {
        page.has_panel()
            && self.panel_open[page.panel_index()]
            && width
                >= RAIL_WIDTH
                    + panel_width
                    + self.main_min(page)
                    + if self.sidebar_open {
                        self.sidebar_width
                    } else {
                        0.
                    }
    }
}

#[cfg(test)]
mod panel_visibility {
    use super::*;

    #[test]
    fn preserves_manual_choice() {
        let mut layout = Layout::default();
        assert!(layout.panel_visible(Page::Files, 1280.));
        assert!(!layout.panel_visible(Page::Files, 900.));
        assert!(layout.panel_visible(Page::Files, 1280.));
        layout.panel_open[1] = false;
        assert!(!layout.panel_visible(Page::Files, 1280.));
        assert!(layout.panel_visible(Page::Git, 1280.));
        assert!(!layout.panel_visible(Page::Conversation, 1280.));
    }

    #[test]
    fn uses_resized_widths() {
        let layout = Layout {
            sidebar_width: 300.,
            panel_width: [400.; 6],
            ..Layout::default()
        };
        assert!(!layout.panel_visible(Page::Git, 1235.));
        assert!(layout.panel_visible(Page::Git, 1236.));
    }
}
