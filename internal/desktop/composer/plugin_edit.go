package composer

type pluginEditState struct {
	composing bool
	selection [2]int
	pending   *[2]int
}

func (state *State) ReplacePluginCommandPrefix(end int, prefix string) bool {
	if state.pluginEdit.composing {
		return false
	}
	runes := []rune(state.Draft)
	if end < 0 || end > len(runes) {
		return false
	}
	state.Draft = prefix + string(runes[end:])
	shift := func(offset int) int { return max(offset-end, 0) + len([]rune(prefix)) }
	selection := [2]int{shift(state.pluginEdit.selection[0]), shift(state.pluginEdit.selection[1])}
	state.pluginEdit.pending = &selection
	state.focus = true
	return true
}
