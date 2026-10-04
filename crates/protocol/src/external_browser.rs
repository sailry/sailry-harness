//! Session-scoped managed browser actions executed on the Node.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    DragAndDrop,
    RightClick,
    Focus,
    ElementState,
    PressKey,
    FileUpload,
    PrintToPdf,
    Click,
    DoubleClick,
    GetCookies,
    GetCookie,
    AddCookie,
    DeleteCookie,
    DeleteAllCookies,
    EvaluateJs,
    Scroll,
    Hover,
    HandleAlert,
    ExtractText,
    ExtractAttribute,
    ExtractLinks,
    PageInfo,
    PageSource,
    SwitchToFrame,
    SwitchToParentFrame,
    SwitchToDefaultContent,
    Navigate,
    Back,
    Forward,
    Refresh,
    Screenshot,
    Type,
    Clear,
    Select,
    WaitForElement,
    Wait,
    WaitForPageLoad,
    WaitForText,
    ListWindows,
    NewTab,
    NewWindow,
    SwitchWindow,
    CloseWindow,
    MaximizeWindow,
    MinimizeWindow,
    SetWindowSize,
    CloseSession,
    Downloads,
    SaveDownload,
}

impl Action {
    /// Read classification follows the browser permission policy, including navigation.
    pub fn read_only(self) -> bool {
        matches!(
            self,
            Self::ExtractText
                | Self::ExtractAttribute
                | Self::ExtractLinks
                | Self::PageInfo
                | Self::PageSource
                | Self::SwitchToFrame
                | Self::SwitchToParentFrame
                | Self::SwitchToDefaultContent
                | Self::Navigate
                | Self::Back
                | Self::Forward
                | Self::Refresh
                | Self::WaitForElement
                | Self::Wait
                | Self::WaitForPageLoad
                | Self::WaitForText
                | Self::ListWindows
                | Self::SwitchWindow
                | Self::Downloads
        )
    }
}
