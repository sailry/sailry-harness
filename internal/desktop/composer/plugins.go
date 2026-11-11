package composer

import "github.com/egoist/mygo/ui"

type StatusCallbacks struct {
	Enabled func(string) bool
	Open    func(string) bool
	SetOpen func(string, bool) bool
	Render  func(*ui.Context, string) bool
}

// ControlPlace is where a slot's package controls are asked for. The
// composer slot has the toolbar's start and end groups, by each control's
// alignment, and the compact settings popover, which takes them all as rows;
// the other slots have one place, ControlStart.
type ControlPlace int

const (
	ControlStart ControlPlace = iota
	ControlEnd
	ControlMenu
)

func (state *State) SetPluginControls(render func(*ui.Context, string, ControlPlace)) {
	state.pluginControls = render
}

func (state *State) SetPluginCommand(command func(Reference) (string, bool)) {
	state.pluginCommand = command
}

func (state *State) SetStatusCallbacks(callbacks *StatusCallbacks) {
	state.statusCallbacks = callbacks
}

func (state *State) renderPluginControls(view *ui.Context, slot string, place ControlPlace) {
	if state.pluginControls != nil {
		state.pluginControls(view, slot, place)
	}
}
