package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/host"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/settings"
)

// hostPage is the chosen host's page data and UI state, made from the
// dataset as first shown.
func (a *App) hostPage() (*host.Host, *host.State) {
	if a.hosts == nil {
		a.hosts, a.hostUI = preview.HostPages(a.Data), map[string]*host.State{}
	}
	st := a.hostUI[a.Host]
	if st == nil {
		st = &host.State{}
		a.hostUI[a.Host] = st
	}
	return a.hosts[a.Host], st
}

// openHost chooses a host and shows its page (Shell::select_host), with
// its update dialog open when update is set.
func (a *App) openHost(id string, update bool) {
	a.Host = id
	a.Navigate(HostPage)
	if update {
		h, st := a.hostPage()
		st.OpenUpdate(h)
	}
}

// addHost is Add host (Shell::add_host): the Connections page's add
// dialog (hosts/add.rs), which settings draws.
func (a *App) addHost() {
	a.Navigate(SettingsPage)
	a.Settings.Open(settings.Connections)
	a.Settings.AddHost()
}

// hostView is the host page in the main column: a project opens its
// first conversation, a session itself.
func (a *App) hostView(c *ui.Context) {
	h, st := a.hostPage()
	host.Page(c, st, h, host.Actions{
		OpenProject: func(id string) {
			for _, pr := range a.Data.Projects {
				if pr.ID == id && len(pr.Conversations) > 0 {
					a.selectConversation(pr.Conversations[0].ID)
				}
			}
		},
		OpenSession: a.selectConversation,
		AddProject:  func() { a.Sidebar.OpenEditor(nil) },
	})
}

// selectConversation shows a conversation, revealing it in the sidebar.
func (a *App) selectConversation(id string) {
	a.Page = ConversationPage
	a.Sidebar.Selected = id
	if pr := a.Data.ProjectOf(id); pr != nil {
		a.Sidebar.Reveal(pr.ID)
	}
}
