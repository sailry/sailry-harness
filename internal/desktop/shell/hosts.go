package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// hostPage is the chosen host's page data and UI state, made from the
// dataset as first shown.
func (a *App) hostPage() (*host.Host, *host.State) {
	if !a.Preview {
		if a.hostUI == nil {
			a.hostUI = map[string]*host.State{}
		}
		if a.hosts == nil {
			a.hosts = map[string]*host.Host{}
		}
		if a.hosts[a.Host] == nil {
			a.hosts[a.Host] = &host.Host{ID: a.Host, Name: "local_host_name", Local: true, Runtime: host.Runtime{Uptime: -1, NodeUptime: -1}}
		}
	}
	if a.Preview && a.hosts == nil {
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
// after a load (Monitor's first sample): the monitor's loading cards and
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

// OpenHost opens a device monitor without changing the selected conversation.
func (a *App) OpenHost(id string) {
	for _, device := range a.Data.Hosts {
		if device.ID == id {
			a.openHost(id, false)
			return
		}
	}
}

// openHost shows one host monitor, with
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

// hostView is the host page in the main column: a running session opens
// its conversation. The page is the host's monitor, so
// the preview's first sample plays while it shows.
func (a *App) hostView(c *ui.Context) {
	h, st := a.hostPage()
	a.sample(c, h)
	host.Page(c, st, h, a.hostActions())
}

// hostActions are the monitored host's actions: the live controller's,
// and the window's own for what
// it leaves out.
func (a *App) hostActions() host.Actions {
	actions := host.Actions{}
	if a.Hooks.HostActions != nil {
		actions = a.Hooks.HostActions(a.Host)
	}
	if actions.OpenSession == nil {
		actions.OpenSession = a.selectConversation
	}
	return actions
}

// selectConversation shows a conversation, revealing it in the sidebar.
func (a *App) selectConversation(id string) {
	a.Page = ConversationPage
	a.Sidebar.Selected = id
	if pr := a.Data.ProjectOf(id); pr != nil {
		a.Sidebar.Reveal(pr.ID)
	}
}

// hostName is a host's name as it shows. The preview names its hosts by i18n
// keys, and the local host is local_host_name until its Node names it. Any
// other name is a device's own: it shows as it is, not as the catalog's text
// for a key it happens to spell.
func (a *App) hostName(c *ui.Context, name string) string {
	if a.Preview || name == "local_host_name" {
		return i18n.T(c, name)
	}
	return name
}

// hostMenu keeps device operations on the monitor for that device.
func (a *App) hostMenu(c *ui.Context, m *kit.Menu) {
	h, st := a.hostPage()
	if h == nil {
		return
	}
	actions := a.hostActions()
	if (a.Hooks.EditHost != nil || !a.Demo()) && m.Item(i18n.T(c, "connection_action_edit")).Chosen() {
		if a.Hooks.EditHost != nil {
			a.Hooks.EditHost(a.Host)
		} else {
			a.notice(c)
		}
	}
	if m.Item(i18n.T(c, "project_add")).Chosen() {
		a.Sidebar.OpenEditorAtHost(a.Host)
	}
	m.Separator()
	m.Submenu(i18n.T(c, "port_mappings"), func(sub *kit.Menu) {
		for _, port := range h.Ports {
			if sub.Item(port.Route()).Disabled(!port.Listening).Chosen() {
				host.OpenPort(c, port, actions)
			}
		}
		if len(h.Ports) > 0 {
			sub.Separator()
		}
		if sub.Item(i18n.T(c, "port_title")).Chosen() {
			st.OpenPorts()
		}
	})
	if h.Update != nil && m.Item(i18n.T(c, "host_update")).Chosen() {
		st.OpenUpdate(h)
	}
	if actions.Revoke != nil {
		m.Separator()
		if m.Item(i18n.T(c, "live_revoke")).Danger().Disabled(h.Revoking).Chosen() {
			st.AskRevoke()
		}
	}
}
