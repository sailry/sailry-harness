package shell

// Usage is the part of the window's state the anonymous feature counters
// compare from one frame to the next. It names no session, project or host.
type Usage struct {
	Page Page
	// PluginPage is the rail feature of the package page shown, "" on any
	// other page.
	PluginPage string
	// Search is whether the search palette is open.
	Search bool
	// ModelPicker is whether a composer's model controls are open.
	ModelPicker bool
	// Splits is how many panes were split off since the window opened.
	Splits int
}

// Usage reads the state after a frame.
func (a *App) Usage() Usage {
	usage := Usage{Page: a.Page, Search: a.search.open, Splits: a.panes.splits}
	if a.Page == PluginPage {
		usage.PluginPage = a.pluginPage
	}
	for _, draft := range a.drafts {
		usage.ModelPicker = usage.ModelPicker || draft.ModelPickerOpen()
	}
	return usage
}
