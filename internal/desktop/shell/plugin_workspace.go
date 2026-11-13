package shell

// PluginWorkspacePrefs belong to one package Workspace. Conversation panel
// widths and other package Workspaces keep their own choices.
type PluginWorkspacePrefs struct {
	Width  float32 `json:"width"`
	Folded bool    `json:"folded,omitempty"`
}

// SetPluginWorkspace projects the mounted Workspace's constraints. A default
// width initializes a Workspace once; subsequent projections preserve drags.
func (a *App) SetPluginWorkspace(key string, minNavigation, minDetails, defaultDetails float32) {
	a.pluginWorkspace = key
	a.pluginNavigationMin = min(max(minNavigation, 0), NavMax)
	a.pluginDetailsMin = min(max(minDetails, 0), 480)
	a.pluginDetailsDefault = min(max(defaultDetails, 260), 480)
	state := a.pluginWorkspacePrefs()
	state.Width = min(max(state.Width, a.pluginDetailsMin), 480)
}

func (a *App) pluginWorkspacePrefs() *PluginWorkspacePrefs {
	if a.pluginWorkspaces == nil {
		a.pluginWorkspaces = map[string]*PluginWorkspacePrefs{}
	}
	state := a.pluginWorkspaces[a.pluginWorkspace]
	if state == nil {
		width := a.pluginDetailsDefault
		if width == 0 {
			width = 400
		}
		state = &PluginWorkspacePrefs{Width: width}
		a.pluginWorkspaces[a.pluginWorkspace] = state
	}
	return state
}

func (a *App) savedPluginWorkspaces() map[string]PluginWorkspacePrefs {
	if len(a.pluginWorkspaces) == 0 {
		return nil
	}
	states := make(map[string]PluginWorkspacePrefs, len(a.pluginWorkspaces))
	for key, state := range a.pluginWorkspaces {
		states[key] = *state
	}
	return states
}

func (a *App) clampNavigation(width float32) float32 {
	if a.Page == PluginPage {
		width = max(width, a.pluginNavigationMin)
	}
	return ClampNav(width)
}

func (a *App) clampPluginDetails(width, window, navigation float32) float32 {
	return max(min(width, 480, window-RailWidth-navigation-MainMin), a.pluginDetailsMin)
}
