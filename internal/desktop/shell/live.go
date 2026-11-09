package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/onboarding"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Hooks keeps I/O and thread scheduling in the live controller.
type Hooks struct {
	BeforeFrame    func(*ui.Context)
	AfterFrame     func()
	Refresh        func()
	OpenFolder     func()
	ReadNotice     func(string)
	ReadNotices    func([]string)
	DismissRead    func([]string)
	DismissNotices func([]string)
}

func (a *App) unavailable(c *ui.Context) {
	kit.Result(c, kit.ResultProps{Title: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
}

func (a *App) notice(c *ui.Context) {
	if a.Preview {
		kit.Message(c, i18n.T(c, "preview_notice"))
		return
	}
	kit.Notify(c, "", i18n.T(c, "unavailable"))
}

// ApplyData replaces a Client projection on the UI thread, retaining local drafts.
func (a *App) ApplyData(d *viewmodel.Data) {
	retain := func(list []*viewmodel.Conversation) {
		for _, cv := range list {
			if len(cv.ID) >= 6 && cv.ID[:6] == "draft/" && d.Find(cv.ID) == nil {
				if old := a.Data.ProjectOf(cv.ID); old != nil {
					for _, pr := range d.Projects {
						if pr.ID == old.ID {
							pr.Conversations = append([]*viewmodel.Conversation{cv}, pr.Conversations...)
						}
					}
				} else {
					d.Unassigned = append([]*viewmodel.Conversation{cv}, d.Unassigned...)
				}
			}
		}
	}
	retain(a.Data.Unassigned)
	for _, p := range a.Data.Projects {
		retain(p.Conversations)
	}
	a.Data = d
	a.Panel.Data = d.Workspace
	a.Sidebar.Data = d
	if d.Find(a.Sidebar.Selected) == nil {
		a.Sidebar.Selected = ""
		if ids := a.Sidebar.Order(); len(ids) > 0 {
			a.Sidebar.Selected = ids[0]
		}
	}
	a.panes.Prune(a.paneExists)
	a.hosts = nil
}

// SetLocalHost supplies current local overview data without preview metrics.
func (a *App) SetLocalHost(h *host.Host) { a.hosts = map[string]*host.Host{h.ID: h} }

// SetWelcome uses the profile preference, independently of window UI state.
func (a *App) SetWelcome(seen bool) { a.Welcome = onboarding.Guide{Seen: seen, Open: !seen} }

// FeaturePreferences returns controller-owned rail order and pin choices.
func (a *App) FeaturePreferences() ([]string, map[string]bool) {
	order := append([]string(nil), a.features...)
	pins := map[string]bool{}
	for _, id := range order {
		pins[id] = !a.unpinned[id]
	}
	return order, pins
}

func (a *App) SetFeaturePreferences(order []string, pins map[string]bool) {
	if len(order) > 0 {
		var merged []string
		seen := map[string]bool{}
		appendID := func(id string) {
			if !seen[id] {
				seen[id] = true
				merged = append(merged, id)
			}
		}
		for _, f := range Features {
			if f.Fixed {
				appendID(f.ID)
			}
		}
		for _, id := range order {
			f, known := featureOf(id)
			if !known || !f.Fixed && !f.Last {
				appendID(id)
			}
		}
		for _, f := range Features {
			if !f.Last {
				appendID(f.ID)
			}
		}
		for _, f := range Features {
			if f.Last {
				appendID(f.ID)
			}
		}
		a.features = merged
	}
	for id, pinned := range pins {
		a.unpinned[id] = !pinned
	}
}
