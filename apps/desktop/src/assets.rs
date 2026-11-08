//! Embedded product SVGs compose with Kit's own assets.
use gpui_kit::{AssetSource, Result, SharedString};
use std::borrow::Cow;

pub(crate) mod file_types;
pub(crate) mod icons;

pub(crate) struct Assets;

// Kit's default source reads checkout files in debug builds. Keep its pinned
// component bundle embedded in portable development packages as well.
gpui_kit::assets::icon_assets!(
    ComponentAssets,
    [
        ALargeSmall,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        ArrowUp,
        Asterisk,
        Ban,
        Battery,
        BatteryCharging,
        BatteryFull,
        BatteryLow,
        BatteryMedium,
        BatteryWarning,
        Bell,
        BookOpen,
        Bot,
        Building2,
        Calendar,
        CaseSensitive,
        ChartPie,
        Check,
        ChevronDown,
        ChevronLeft,
        ChevronRight,
        ChevronUp,
        ChevronsUpDown,
        CircleAlert,
        CircleCheck,
        CircleUser,
        CircleX,
        Close,
        Copy,
        Cpu,
        Dash,
        Delete,
        Ellipsis,
        EllipsisVertical,
        ExternalLink,
        Eye,
        EyeOff,
        File,
        FileText,
        Folder,
        FolderClosed,
        FolderOpen,
        Frame,
        GalleryVerticalEnd,
        GitBranch,
        Github,
        Globe,
        HardDrive,
        Heart,
        HeartOff,
        Inbox,
        Info,
        Inspector,
        LayoutDashboard,
        Loader,
        LoaderCircle,
        Map,
        Maximize,
        MemoryStick,
        Menu,
        Mic,
        Minimize,
        Minus,
        Moon,
        Network,
        Palette,
        PanelBottom,
        PanelBottomOpen,
        PanelLeft,
        PanelLeftClose,
        PanelLeftOpen,
        PanelRight,
        PanelRightClose,
        PanelRightOpen,
        Pause,
        Pin,
        PinOff,
        Play,
        Plus,
        Redo,
        Redo2,
        RefreshCw,
        Replace,
        ResizeCorner,
        RotateCw,
        Search,
        Settings,
        Settings2,
        SortAscending,
        SortDescending,
        Square,
        SquareTerminal,
        Star,
        StarFill,
        StarOff,
        Sun,
        ThumbsDown,
        ThumbsUp,
        TriangleAlert,
        Undo,
        Undo2,
        User,
        WindowClose,
        WindowMaximize,
        WindowMinimize,
        WindowRestore,
    ]
);

gpui_kit::assets::icon_assets!(
    ActivityAssets,
    [
        ListTodo,
        CheckCheck,
        CircleCheck,
        ShieldAlert,
        MessageCircleQuestionMark
    ]
);
gpui_kit::assets::icon_assets!(UsageAssets, [Gauge, DollarSign, MessagesSquare, Coins]);
gpui_kit::assets::icon_assets!(
    PickerAssets,
    [Monitor, List, LayoutGrid, FolderInput, FileArchive]
);
gpui_kit::assets::icon_assets!(
    SettingsAssets,
    [
        Download,
        Upload,
        Trash,
        Volume2,
        Square,
        Pause,
        House,
        RotateCcw,
        Package,
        Box,
        Code,
        Clock,
        CircleQuestionMark,
        KeyRound,
        Link,
        Cloud,
        Database,
        Users,
        Crown,
        Shield,
        Wrench,
        Server,
        Tag,
        Plug,
        BrainCircuit,
        Zap
    ]
);

// Plugins may name any bundled Lucide icon, which the full catalog serves. In
// debug builds that catalog reads checkout files, so keep the editor icons
// outside Kit's default set embedded for portable development packages.
gpui_kit::assets::icon_assets!(
    ContentAssets,
    [
        Bold,
        Italic,
        Strikethrough,
        ListOrdered,
        ListChecks,
        Quote,
        Table,
        TextWrap,
        CodeXml,
        Image,
        ImagePlus,
        SeparatorHorizontal,
        SquareCode,
        Workflow,
        Sigma,
        ChartBar,
        FileCode,
        X
    ]
);

pub(crate) const DRAG_ICON: &str = "icons/reicon/move.svg";

// Kit b79f4ce has no file-diff or Git status glyphs. These product SVGs use
// Kit Icon rendering, semantic colors and sizing; they are not custom controls.
// Kit b79f4ce also lacks save, cut and paste glyphs; use product SVGs through Kit Icon.
// Kit b79f4ce lacks permission shields; use unchanged Reicon Outline assets via Kit Icon.
// Kit b79f4ce has no microphone glyph; use the unchanged Reicon asset through Kit Icon.
// Kit b79f4ce lacks mirror glyphs; use product SVGs through Kit Icon.
// Kit b79f4ce has no drag handle; use Reicon’s original four-arrow glyph with the grab cursor.
const ICONS: &[(&str, &[u8])] = &[
    (
        "branding/sailry.png",
        include_bytes!("../../../assets/branding/sailry.png"),
    ),
    (
        "branding/sailry-mark.svg",
        include_bytes!("../../../assets/branding/sailry-mark.svg"),
    ),
    (
        "branding/sailry-mark-white.svg",
        include_bytes!("../../../assets/branding/sailry-mark-white.svg"),
    ),
    (
        "icons/remix/progress-2-line.svg",
        include_bytes!("../../../assets/icons/remix/progress-2-line.svg"),
    ),
    (
        "icons/remix/progress-4-line.svg",
        include_bytes!("../../../assets/icons/remix/progress-4-line.svg"),
    ),
    (
        "icons/remix/progress-6-line.svg",
        include_bytes!("../../../assets/icons/remix/progress-6-line.svg"),
    ),
    (
        "icons/remix/checkbox-blank-circle-line.svg",
        include_bytes!("../../../assets/icons/remix/checkbox-blank-circle-line.svg"),
    ),
    (
        "icons/remix/close-circle-line.svg",
        include_bytes!("../../../assets/icons/remix/close-circle-line.svg"),
    ),
    (
        "icons/files/word.svg",
        include_bytes!("../../../assets/icons/files/word.svg"),
    ),
    (
        "icons/files/excel.svg",
        include_bytes!("../../../assets/icons/files/excel.svg"),
    ),
    (
        "icons/files/powerpoint.svg",
        include_bytes!("../../../assets/icons/files/powerpoint.svg"),
    ),
    (
        "icons/files/pdf.svg",
        include_bytes!("../../../assets/icons/files/pdf.svg"),
    ),
    (
        "icons/reicon/gift.svg",
        include_bytes!("../../../assets/icons/reicon/gift.svg"),
    ),
    (
        "icons/reicon/receipt.svg",
        include_bytes!("../../../assets/icons/reicon/receipt.svg"),
    ),
    (
        "icons/reicon/cart.svg",
        include_bytes!("../../../assets/icons/reicon/cart.svg"),
    ),
    (
        "icons/reicon/handshake.svg",
        include_bytes!("../../../assets/icons/reicon/handshake.svg"),
    ),
    (
        DRAG_ICON,
        include_bytes!("../../../assets/icons/reicon/move.svg"),
    ),
    (
        "icons/flip-horizontal.svg",
        include_bytes!("../../../assets/icons/flip-horizontal.svg"),
    ),
    (
        "icons/flip-vertical.svg",
        include_bytes!("../../../assets/icons/flip-vertical.svg"),
    ),
    (
        "icons/reicon/microphone.svg",
        include_bytes!("../../../assets/icons/reicon/microphone.svg"),
    ),
    (
        "icons/providers/opencode.svg",
        include_bytes!("../../../assets/icons/providers/opencode.svg"),
    ),
    (
        "icons/providers/openai.svg",
        include_bytes!("../../../assets/icons/providers/openai.svg"),
    ),
    (
        "icons/providers/anthropic.svg",
        include_bytes!("../../../assets/icons/providers/anthropic.svg"),
    ),
    (
        "icons/providers/gemini.svg",
        include_bytes!("../../../assets/icons/providers/gemini.svg"),
    ),
    (
        "icons/providers/xai.svg",
        include_bytes!("../../../assets/icons/providers/xai.svg"),
    ),
    (
        "icons/providers/deepseek.svg",
        include_bytes!("../../../assets/icons/providers/deepseek.svg"),
    ),
    (
        "icons/providers/qwen.svg",
        include_bytes!("../../../assets/icons/providers/qwen.svg"),
    ),
    (
        "icons/providers/kimi.svg",
        include_bytes!("../../../assets/icons/providers/kimi.svg"),
    ),
    (
        "icons/providers/mistral.svg",
        include_bytes!("../../../assets/icons/providers/mistral.svg"),
    ),
    (
        "icons/providers/minimax.svg",
        include_bytes!("../../../assets/icons/providers/minimax.svg"),
    ),
    (
        "icons/providers/doubao.svg",
        include_bytes!("../../../assets/icons/providers/doubao.svg"),
    ),
    (
        "icons/providers/zai.svg",
        include_bytes!("../../../assets/icons/providers/zai.svg"),
    ),
    (
        "icons/providers/wenxin.svg",
        include_bytes!("../../../assets/icons/providers/wenxin.svg"),
    ),
    (
        "icons/providers/cohere.svg",
        include_bytes!("../../../assets/icons/providers/cohere.svg"),
    ),
    (
        "icons/reicon/shield.svg",
        include_bytes!("../../../assets/icons/reicon/shield.svg"),
    ),
    (
        "icons/reicon/shield-off.svg",
        include_bytes!("../../../assets/icons/reicon/shield-off.svg"),
    ),
    (
        "icons/os/android.svg",
        include_bytes!("../../../assets/icons/os/android.svg"),
    ),
    (
        "icons/os/apple.svg",
        include_bytes!("../../../assets/icons/os/apple.svg"),
    ),
    (
        "icons/os/windows11.svg",
        include_bytes!("../../../assets/icons/os/windows11.svg"),
    ),
    (
        "icons/os/linux.svg",
        include_bytes!("../../../assets/icons/os/linux.svg"),
    ),
    (
        "icons/fluent/document.svg",
        include_bytes!("../../../assets/icons/fluent/document.svg"),
    ),
    (
        "icons/fluent/document_text.svg",
        include_bytes!("../../../assets/icons/fluent/document_text.svg"),
    ),
    (
        "icons/fluent/code_block.svg",
        include_bytes!("../../../assets/icons/fluent/code_block.svg"),
    ),
    (
        "icons/fluent/table.svg",
        include_bytes!("../../../assets/icons/fluent/table.svg"),
    ),
    (
        "icons/fluent/slide_text_sparkle.svg",
        include_bytes!("../../../assets/icons/fluent/slide_text_sparkle.svg"),
    ),
    (
        "icons/fluent/document_folder.svg",
        include_bytes!("../../../assets/icons/fluent/document_folder.svg"),
    ),
    (
        "icons/save.svg",
        include_bytes!("../../../assets/icons/save.svg"),
    ),
    (
        "icons/cut.svg",
        include_bytes!("../../../assets/icons/cut.svg"),
    ),
    (
        "icons/paste.svg",
        include_bytes!("../../../assets/icons/paste.svg"),
    ),
    (
        "icons/git-added.svg",
        include_bytes!("../../../assets/icons/git-added.svg"),
    ),
    (
        "icons/git-modified.svg",
        include_bytes!("../../../assets/icons/git-modified.svg"),
    ),
    (
        "icons/git-deleted.svg",
        include_bytes!("../../../assets/icons/git-deleted.svg"),
    ),
    (
        "icons/git-renamed.svg",
        include_bytes!("../../../assets/icons/git-renamed.svg"),
    ),
    (
        "icons/git-conflict.svg",
        include_bytes!("../../../assets/icons/git-conflict.svg"),
    ),
    (
        "icons/file-diff.svg",
        include_bytes!("../../../assets/icons/file-diff.svg"),
    ),
    (
        "icons/reicon/accessibility.svg",
        include_bytes!("../../../assets/icons/reicon/accessibility.svg"),
    ),
    (
        "icons/reicon/screen-recording.svg",
        include_bytes!("../../../assets/icons/reicon/screen-recording.svg"),
    ),
    (
        "icons/reicon/smartphone.svg",
        include_bytes!("../../../assets/icons/reicon/smartphone.svg"),
    ),
    (
        "icons/reicon/help-circle.svg",
        include_bytes!("../../../assets/icons/reicon/help-circle.svg"),
    ),
    (
        "icons/reicon/paperclip.svg",
        include_bytes!("../../../assets/icons/reicon/paperclip.svg"),
    ),
    (
        "icons/reicon/folder.svg",
        include_bytes!("../../../assets/icons/reicon/folder.svg"),
    ),
    (
        "icons/reicon/code.svg",
        include_bytes!("../../../assets/icons/reicon/code.svg"),
    ),
    (
        "icons/reicon/terminal.svg",
        include_bytes!("../../../assets/icons/reicon/terminal.svg"),
    ),
    (
        "icons/reicon/globe.svg",
        include_bytes!("../../../assets/icons/reicon/globe.svg"),
    ),
    (
        "icons/reicon/server.svg",
        include_bytes!("../../../assets/icons/reicon/server.svg"),
    ),
    (
        "icons/reicon/database.svg",
        include_bytes!("../../../assets/icons/reicon/database.svg"),
    ),
    (
        "icons/reicon/package.svg",
        include_bytes!("../../../assets/icons/reicon/package.svg"),
    ),
    (
        "icons/reicon/game.svg",
        include_bytes!("../../../assets/icons/reicon/game.svg"),
    ),
    (
        "icons/reicon/book.svg",
        include_bytes!("../../../assets/icons/reicon/book.svg"),
    ),
    (
        "icons/reicon/finance.svg",
        include_bytes!("../../../assets/icons/reicon/finance.svg"),
    ),
    (
        "icons/reicon/device.svg",
        include_bytes!("../../../assets/icons/reicon/device.svg"),
    ),
    (
        "icons/reicon/education.svg",
        include_bytes!("../../../assets/icons/reicon/education.svg"),
    ),
    (
        "icons/reicon/writing.svg",
        include_bytes!("../../../assets/icons/reicon/writing.svg"),
    ),
    (
        "icons/reicon/tags.svg",
        include_bytes!("../../../assets/icons/reicon/tags.svg"),
    ),
    (
        "icons/reicon/music.svg",
        include_bytes!("../../../assets/icons/reicon/music.svg"),
    ),
    (
        "icons/reicon/media.svg",
        include_bytes!("../../../assets/icons/reicon/media.svg"),
    ),
    (
        "icons/reicon/design.svg",
        include_bytes!("../../../assets/icons/reicon/design.svg"),
    ),
    (
        "icons/reicon/health.svg",
        include_bytes!("../../../assets/icons/reicon/health.svg"),
    ),
    (
        "icons/reicon/nature.svg",
        include_bytes!("../../../assets/icons/reicon/nature.svg"),
    ),
    (
        "icons/reicon/business.svg",
        include_bytes!("../../../assets/icons/reicon/business.svg"),
    ),
    (
        "icons/reicon/analytics.svg",
        include_bytes!("../../../assets/icons/reicon/analytics.svg"),
    ),
    (
        "icons/reicon/profile.svg",
        include_bytes!("../../../assets/icons/reicon/profile.svg"),
    ),
    (
        "icons/reicon/fitness.svg",
        include_bytes!("../../../assets/icons/reicon/fitness.svg"),
    ),
    (
        "icons/reicon/law.svg",
        include_bytes!("../../../assets/icons/reicon/law.svg"),
    ),
    (
        "icons/reicon/audio.svg",
        include_bytes!("../../../assets/icons/reicon/audio.svg"),
    ),
    (
        "icons/reicon/travel.svg",
        include_bytes!("../../../assets/icons/reicon/travel.svg"),
    ),
    (
        "icons/reicon/tools.svg",
        include_bytes!("../../../assets/icons/reicon/tools.svg"),
    ),
    (
        "icons/reicon/science.svg",
        include_bytes!("../../../assets/icons/reicon/science.svg"),
    ),
    (
        "icons/reicon/ai.svg",
        include_bytes!("../../../assets/icons/reicon/ai.svg"),
    ),
    (
        "icons/reicon/favorite.svg",
        include_bytes!("../../../assets/icons/reicon/favorite.svg"),
    ),
];

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = ICONS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = file_types::load(path) {
            return Ok(Some(Cow::Owned(bytes)));
        }
        if let Some(bytes) = icons::load(path) {
            return Ok(Some(Cow::Owned(bytes)));
        }
        if let Some(bytes) = ActivityAssets.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = UsageAssets.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = SettingsAssets.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = PickerAssets.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = ComponentAssets.load(path)? {
            return Ok(Some(bytes));
        }
        if let Some(bytes) = ContentAssets.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut names = gpui_kit::assets::AllAssets.list(path)?;
        names.extend(ComponentAssets.list(path)?);
        names.extend(ContentAssets.list(path)?);
        names.extend(ActivityAssets.list(path)?);
        names.extend(UsageAssets.list(path)?);
        names.extend(SettingsAssets.list(path)?);
        names.extend(PickerAssets.list(path)?);
        names.extend(
            ICONS
                .iter()
                .filter(|(name, _)| name.starts_with(path))
                .map(|(name, _)| (*name).into()),
        );
        names.extend(icons::list(path));
        names.extend(file_types::list(path));
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_inventory() {
        for (scenario, check) in [
            (
                "embedded_component_icons",
                (|_scenario: &str| {
                    let paths = ComponentAssets.list("icons/").unwrap();
                    assert_eq!(paths.len(), 109, "embedded component icons");
                    let names = Assets.list("icons/").unwrap();
                    for path in paths {
                        let bytes = ComponentAssets.load(&path).unwrap().unwrap();
                        assert!(
                            matches!(bytes, Cow::Borrowed(_)),
                            "embedded component icons: {path}"
                        );
                        assert!(!bytes.is_empty(), "embedded component icons: {path}");
                        assert_eq!(
                            Assets.load(&path).unwrap().unwrap(),
                            bytes,
                            "embedded component icons: {path}"
                        );
                        assert!(names.contains(&path), "embedded component icons: {path}");
                    }
                }) as fn(&str),
            ),
            (
                "embedded_product_icons",
                (|_scenario: &str| {
                    assert!(
                        Assets.load("icons/folder.svg").unwrap().is_some(),
                        "embedded product icons"
                    );
                    assert!(
                        Assets.load("icons/list-todo.svg").unwrap().is_some(),
                        "embedded product icons"
                    );
                }),
            ),
            (
                "footer_icons",
                (|_scenario: &str| {
                    let names = Assets.list("icons/").unwrap();
                    for path in [
                        "icons/gauge.svg",
                        "icons/dollar-sign.svg",
                        "icons/messages-square.svg",
                    ] {
                        let expected = UsageAssets.load(path).unwrap().unwrap();
                        assert_eq!(
                            Assets.load(path).unwrap().unwrap(),
                            expected,
                            "footer icons"
                        );
                        assert!(
                            names.iter().any(|name| name.as_ref() == path),
                            "footer icons"
                        );
                    }
                }),
            ),
            (
                "settings_icons",
                (|_scenario: &str| {
                    let names = Assets.list("icons/").unwrap();
                    for path in [
                        "icons/download.svg",
                        "icons/upload.svg",
                        "icons/trash.svg",
                        "icons/volume-2.svg",
                        "icons/square.svg",
                        "icons/pause.svg",
                        "icons/house.svg",
                        "icons/rotate-ccw.svg",
                        "icons/key-round.svg",
                        "icons/link.svg",
                        "icons/cloud.svg",
                        "icons/plug.svg",
                        "icons/brain-circuit.svg",
                        "icons/zap.svg",
                        "icons/crown.svg",
                    ] {
                        let expected = SettingsAssets.load(path).unwrap().unwrap();
                        assert_eq!(
                            Assets.load(path).unwrap().unwrap(),
                            expected,
                            "settings icons"
                        );
                        assert!(
                            names.iter().any(|name| name.as_ref() == path),
                            "settings icons"
                        );
                    }
                }),
            ),
            (
                "content_icons",
                (|_scenario: &str| {
                    let names = Assets.list("icons/").unwrap();
                    for path in [
                        "icons/bold.svg",
                        "icons/italic.svg",
                        "icons/strikethrough.svg",
                        "icons/list-ordered.svg",
                        "icons/list-checks.svg",
                        "icons/quote.svg",
                        "icons/table.svg",
                        "icons/text-wrap.svg",
                        "icons/code-xml.svg",
                        "icons/image.svg",
                        "icons/image-plus.svg",
                        "icons/separator-horizontal.svg",
                        "icons/square-code.svg",
                        "icons/workflow.svg",
                        "icons/sigma.svg",
                        "icons/chart-bar.svg",
                        "icons/file-code.svg",
                        "icons/x.svg",
                    ] {
                        assert_eq!(
                            Assets.load(path).unwrap().unwrap(),
                            ContentAssets.load(path).unwrap().unwrap(),
                            "content icons"
                        );
                        assert!(
                            names.iter().any(|name| name.as_ref() == path),
                            "content icons"
                        );
                    }
                }),
            ),
            (
                "catalog_icons",
                (|_scenario: &str| {
                    assert!(
                        Assets.load("icons/heading-1.svg").unwrap().is_some(),
                        "catalog icons"
                    );
                    assert!(
                        Assets.load("icons/missing-icon.svg").is_err(),
                        "catalog icons"
                    );
                }),
            ),
            (
                "picker_icons",
                (|_scenario: &str| {
                    let names = Assets.list("icons/").unwrap();
                    for path in [
                        "icons/monitor.svg",
                        "icons/list.svg",
                        "icons/layout-grid.svg",
                        "icons/file-archive.svg",
                    ] {
                        assert_eq!(
                            Assets.load(path).unwrap().unwrap(),
                            PickerAssets.load(path).unwrap().unwrap(),
                            "picker icons"
                        );
                        assert!(
                            names.iter().any(|name| name.as_ref() == path),
                            "picker icons"
                        );
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
