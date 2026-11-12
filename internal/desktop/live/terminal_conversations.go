package live

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
)

// conversationPanelsFrame keeps the conversations' right panels in step with
// the conversations: the selected one
// is mounted, a browser page opened for a conversation that is not shown goes
// to that conversation's panel, and the panel of a conversation that is gone
// leaves with it, its terminals closed on their Node.
func (r *Runtime) conversationPanelsFrame() {
	if r.parent != nil {
		return
	}
	app := r.App
	app.MountPanel()
	if r.browser != nil {
		for _, tab := range app.ConversationTabs(app.Sidebar.Selected) {
			if tab.Kind != panel.Browser {
				continue
			}
			if page := r.browser.tabs[tab.ID]; page != nil && page.conversation != "" && page.conversation != app.Sidebar.Selected {
				app.MovePanelTab(tab, page.conversation)
			}
		}
	}
	settled := r.terminalOwnersSettled()
	for _, id := range app.PanelConversations() {
		if id == app.Sidebar.Selected {
			continue
		}
		if strings.HasPrefix(id, "draft/") {
			// An unsent chat is the window's own: it is there or removed.
			if app.Data.Find(id) != nil {
				continue
			}
		} else if !settled || r.conversationKnown(id) {
			// Its Node cannot say yet, or still lists it unarchived.
			continue
		}
		for _, tab := range app.ConversationTabs(id) {
			if tab.Kind == panel.Terminal {
				r.closeLiveTerminal(tab.ID)
			}
		}
		app.DropPanel(id)
	}
}

// conversationKnown reports whether a Node lists session id and has not
// archived it.
func (r *Runtime) conversationKnown(id string) bool {
	for _, owner := range r.terminalOwners() {
		if session, exists := owner.session(id); exists && !session.Archived {
			return true
		}
	}
	return false
}
