package panel

import "github.com/egoist/mygo/ui"

func (state *State) SetPluginResourceView(render func(*ui.Context, Tab) bool) {
	state.pluginResource = render
}

func (state *State) SetPluginResourceCreate(create func(Kind) bool) {
	state.pluginCreateResource = create
}

func (state *State) SetPluginResourceClose(close func(Tab) bool) {
	state.pluginCloseResource = close
}

func (state *State) SetPluginResourceTitle(title func(Tab) (string, bool)) {
	state.pluginResourceTitle = title
}
