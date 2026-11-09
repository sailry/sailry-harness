// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
	"fmt"
)

type ExternalBrowserAction string

const (
	ExternalBrowserActionDragAndDrop            ExternalBrowserAction = "drag_and_drop"
	ExternalBrowserActionRightClick             ExternalBrowserAction = "right_click"
	ExternalBrowserActionFocus                  ExternalBrowserAction = "focus"
	ExternalBrowserActionElementState           ExternalBrowserAction = "element_state"
	ExternalBrowserActionPressKey               ExternalBrowserAction = "press_key"
	ExternalBrowserActionFileUpload             ExternalBrowserAction = "file_upload"
	ExternalBrowserActionPrintToPdf             ExternalBrowserAction = "print_to_pdf"
	ExternalBrowserActionClick                  ExternalBrowserAction = "click"
	ExternalBrowserActionDoubleClick            ExternalBrowserAction = "double_click"
	ExternalBrowserActionGetCookies             ExternalBrowserAction = "get_cookies"
	ExternalBrowserActionGetCookie              ExternalBrowserAction = "get_cookie"
	ExternalBrowserActionAddCookie              ExternalBrowserAction = "add_cookie"
	ExternalBrowserActionDeleteCookie           ExternalBrowserAction = "delete_cookie"
	ExternalBrowserActionDeleteAllCookies       ExternalBrowserAction = "delete_all_cookies"
	ExternalBrowserActionEvaluateJs             ExternalBrowserAction = "evaluate_js"
	ExternalBrowserActionScroll                 ExternalBrowserAction = "scroll"
	ExternalBrowserActionHover                  ExternalBrowserAction = "hover"
	ExternalBrowserActionHandleAlert            ExternalBrowserAction = "handle_alert"
	ExternalBrowserActionExtractText            ExternalBrowserAction = "extract_text"
	ExternalBrowserActionExtractAttribute       ExternalBrowserAction = "extract_attribute"
	ExternalBrowserActionExtractLinks           ExternalBrowserAction = "extract_links"
	ExternalBrowserActionPageInfo               ExternalBrowserAction = "page_info"
	ExternalBrowserActionPageSource             ExternalBrowserAction = "page_source"
	ExternalBrowserActionSwitchToFrame          ExternalBrowserAction = "switch_to_frame"
	ExternalBrowserActionSwitchToParentFrame    ExternalBrowserAction = "switch_to_parent_frame"
	ExternalBrowserActionSwitchToDefaultContent ExternalBrowserAction = "switch_to_default_content"
	ExternalBrowserActionNavigate               ExternalBrowserAction = "navigate"
	ExternalBrowserActionBack                   ExternalBrowserAction = "back"
	ExternalBrowserActionForward                ExternalBrowserAction = "forward"
	ExternalBrowserActionRefresh                ExternalBrowserAction = "refresh"
	ExternalBrowserActionScreenshot             ExternalBrowserAction = "screenshot"
	ExternalBrowserActionType                   ExternalBrowserAction = "type"
	ExternalBrowserActionClear                  ExternalBrowserAction = "clear"
	ExternalBrowserActionSelect                 ExternalBrowserAction = "select"
	ExternalBrowserActionWaitForElement         ExternalBrowserAction = "wait_for_element"
	ExternalBrowserActionWait                   ExternalBrowserAction = "wait"
	ExternalBrowserActionWaitForPageLoad        ExternalBrowserAction = "wait_for_page_load"
	ExternalBrowserActionWaitForText            ExternalBrowserAction = "wait_for_text"
	ExternalBrowserActionListWindows            ExternalBrowserAction = "list_windows"
	ExternalBrowserActionNewTab                 ExternalBrowserAction = "new_tab"
	ExternalBrowserActionNewWindow              ExternalBrowserAction = "new_window"
	ExternalBrowserActionSwitchWindow           ExternalBrowserAction = "switch_window"
	ExternalBrowserActionCloseWindow            ExternalBrowserAction = "close_window"
	ExternalBrowserActionMaximizeWindow         ExternalBrowserAction = "maximize_window"
	ExternalBrowserActionMinimizeWindow         ExternalBrowserAction = "minimize_window"
	ExternalBrowserActionSetWindowSize          ExternalBrowserAction = "set_window_size"
	ExternalBrowserActionCloseSession           ExternalBrowserAction = "close_session"
	ExternalBrowserActionDownloads              ExternalBrowserAction = "downloads"
	ExternalBrowserActionSaveDownload           ExternalBrowserAction = "save_download"
)

func (value ExternalBrowserAction) MarshalJSON() ([]byte, error) {
	switch value {
	case ExternalBrowserActionDragAndDrop, ExternalBrowserActionRightClick, ExternalBrowserActionFocus, ExternalBrowserActionElementState, ExternalBrowserActionPressKey, ExternalBrowserActionFileUpload, ExternalBrowserActionPrintToPdf, ExternalBrowserActionClick, ExternalBrowserActionDoubleClick, ExternalBrowserActionGetCookies, ExternalBrowserActionGetCookie, ExternalBrowserActionAddCookie, ExternalBrowserActionDeleteCookie, ExternalBrowserActionDeleteAllCookies, ExternalBrowserActionEvaluateJs, ExternalBrowserActionScroll, ExternalBrowserActionHover, ExternalBrowserActionHandleAlert, ExternalBrowserActionExtractText, ExternalBrowserActionExtractAttribute, ExternalBrowserActionExtractLinks, ExternalBrowserActionPageInfo, ExternalBrowserActionPageSource, ExternalBrowserActionSwitchToFrame, ExternalBrowserActionSwitchToParentFrame, ExternalBrowserActionSwitchToDefaultContent, ExternalBrowserActionNavigate, ExternalBrowserActionBack, ExternalBrowserActionForward, ExternalBrowserActionRefresh, ExternalBrowserActionScreenshot, ExternalBrowserActionType, ExternalBrowserActionClear, ExternalBrowserActionSelect, ExternalBrowserActionWaitForElement, ExternalBrowserActionWait, ExternalBrowserActionWaitForPageLoad, ExternalBrowserActionWaitForText, ExternalBrowserActionListWindows, ExternalBrowserActionNewTab, ExternalBrowserActionNewWindow, ExternalBrowserActionSwitchWindow, ExternalBrowserActionCloseWindow, ExternalBrowserActionMaximizeWindow, ExternalBrowserActionMinimizeWindow, ExternalBrowserActionSetWindowSize, ExternalBrowserActionCloseSession, ExternalBrowserActionDownloads, ExternalBrowserActionSaveDownload:
		return json.Marshal(string(value))
	default:
		return nil, fmt.Errorf("unknown ExternalBrowserAction %q", value)
	}
}
func (value *ExternalBrowserAction) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ExternalBrowserAction(decoded) {
	case ExternalBrowserActionDragAndDrop, ExternalBrowserActionRightClick, ExternalBrowserActionFocus, ExternalBrowserActionElementState, ExternalBrowserActionPressKey, ExternalBrowserActionFileUpload, ExternalBrowserActionPrintToPdf, ExternalBrowserActionClick, ExternalBrowserActionDoubleClick, ExternalBrowserActionGetCookies, ExternalBrowserActionGetCookie, ExternalBrowserActionAddCookie, ExternalBrowserActionDeleteCookie, ExternalBrowserActionDeleteAllCookies, ExternalBrowserActionEvaluateJs, ExternalBrowserActionScroll, ExternalBrowserActionHover, ExternalBrowserActionHandleAlert, ExternalBrowserActionExtractText, ExternalBrowserActionExtractAttribute, ExternalBrowserActionExtractLinks, ExternalBrowserActionPageInfo, ExternalBrowserActionPageSource, ExternalBrowserActionSwitchToFrame, ExternalBrowserActionSwitchToParentFrame, ExternalBrowserActionSwitchToDefaultContent, ExternalBrowserActionNavigate, ExternalBrowserActionBack, ExternalBrowserActionForward, ExternalBrowserActionRefresh, ExternalBrowserActionScreenshot, ExternalBrowserActionType, ExternalBrowserActionClear, ExternalBrowserActionSelect, ExternalBrowserActionWaitForElement, ExternalBrowserActionWait, ExternalBrowserActionWaitForPageLoad, ExternalBrowserActionWaitForText, ExternalBrowserActionListWindows, ExternalBrowserActionNewTab, ExternalBrowserActionNewWindow, ExternalBrowserActionSwitchWindow, ExternalBrowserActionCloseWindow, ExternalBrowserActionMaximizeWindow, ExternalBrowserActionMinimizeWindow, ExternalBrowserActionSetWindowSize, ExternalBrowserActionCloseSession, ExternalBrowserActionDownloads, ExternalBrowserActionSaveDownload:
		*value = ExternalBrowserAction(decoded)
		return nil
	default:
		return fmt.Errorf("unknown ExternalBrowserAction %q", decoded)
	}
}
func (value ExternalBrowserAction) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ExternalBrowserAction) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
