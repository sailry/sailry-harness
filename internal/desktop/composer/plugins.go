package composer

import "github.com/egoist/mygo/ui"

type StatusCallbacks struct {
	Enabled func(string) bool
	Open    func(string) bool
	SetOpen func(string, bool) bool
	Render  func(*ui.Context, string) bool
}

func (state *State) SetPluginControls(render func(*ui.Context, string)) {
	state.pluginControls = render
}

func (state *State) SetPluginCommand(command func(Reference) (string, bool)) {
	state.pluginCommand = command
}

func (state *State) SetStatusCallbacks(callbacks *StatusCallbacks) {
	state.statusCallbacks = callbacks
}

func (state *State) renderPluginControls(view *ui.Context, slot string) {
	if state.pluginControls != nil {
		state.pluginControls(view, slot)
	}
}
