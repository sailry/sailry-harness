package conversation

import "github.com/egoist/mygo/ui"

func (state *State) SetToolCard(sessionID, callID string, render func(*ui.Context)) {
	if render == nil {
		if calls := state.toolCards[sessionID]; calls != nil {
			delete(calls, callID)
			if len(calls) == 0 {
				delete(state.toolCards, sessionID)
			}
		}
		return
	}
	if state.toolCards == nil {
		state.toolCards = map[string]map[string]func(*ui.Context){}
	}
	if state.toolCards[sessionID] == nil {
		state.toolCards[sessionID] = map[string]func(*ui.Context){}
	}
	state.toolCards[sessionID][callID] = render
}
