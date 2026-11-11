package protocol

// ReadOnly reports whether an action observes or moves between pages without
// approval. Every other action is a control action.
func (value ExternalBrowserAction) ReadOnly() bool {
	switch value {
	case ExternalBrowserActionExtractText, ExternalBrowserActionExtractAttribute, ExternalBrowserActionExtractLinks,
		ExternalBrowserActionPageInfo, ExternalBrowserActionPageSource,
		ExternalBrowserActionSwitchToFrame, ExternalBrowserActionSwitchToParentFrame, ExternalBrowserActionSwitchToDefaultContent,
		ExternalBrowserActionNavigate, ExternalBrowserActionBack, ExternalBrowserActionForward, ExternalBrowserActionRefresh,
		ExternalBrowserActionWaitForElement, ExternalBrowserActionWait, ExternalBrowserActionWaitForPageLoad, ExternalBrowserActionWaitForText,
		ExternalBrowserActionListWindows, ExternalBrowserActionSwitchWindow, ExternalBrowserActionDownloads:
		return true
	}
	return false
}
