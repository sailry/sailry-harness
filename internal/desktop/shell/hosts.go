package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// hostPage is the chosen host's page data and UI state, made from the
// dataset as first shown.
func (a *App) hostPage() (*host.Host, *host.State) {
	if a.hosts == nil {
		a.hosts, a.hostUI, a.samples = preview.HostPages(a.Data), map[string]*host.State{}, map[string]*sample{}
		for id, h := range a.hosts {
			x := &sample{metrics: h.Metrics}
			x.fetch.Start()
			a.samples[id], h.Metrics = x, nil
		}
	}
	st := a.hostUI[a.Host]
	if st == nil {
		st = &host.State{}
		a.hostUI[a.Host] = st
	}
	return a.hosts[a.Host], st
}

// sample is a host's first metrics sample, which the preview delivers
// after a load (Monitor's first sample): the metrics' loading state and
// the processes' skeleton show until then.
type sample struct {
	fetch   preview.Fetch
	metrics *host.Metrics
}

// sample plays the host's first sample while its metrics show.
func (a *App) sample(c *ui.Context, h *host.Host) {
	x := a.samples[h.ID]
	if x == nil || !x.fetch.Loading {
		return
	}
	if x.fetch.Advance(c.Now()) {
		c.After(preview.Tick)
		return
	}
	h.Metrics = x.metrics
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

// addHost is Add host (Shell::add_host): the Connections page's add dialog,
// which settings draws.
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
