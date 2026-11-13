package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/onboarding"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"slices"
)

// Hooks keeps I/O and thread scheduling in the live controller.
type Hooks struct {
	HostActions      func(string) host.Actions
	EditHost         func(string)
	WorkspaceProject func() *panel.Project
	DownloadDialog   func(*ui.Context)
	PackageDialogs   func(*ui.Context)
	// FilePreview draws the preview dialog of a file outside a
	// conversation.
	FilePreview func(*ui.Context)
	BeforeFrame func(*ui.Context)
	AfterFrame  func()
	// DemoChanged runs when Demo mode went on, off or into another
	// language, before the frame's other hooks.
	DemoChanged      func()
	Refresh          func()
	ReadNotice       func(string)
	ReadNotices      func([]string)
	DismissRead      func([]string)
	DismissNotices   func([]string)
	OpenKnowledge    func(c *ui.Context, node string, source Source)
	CaptureSelection func()
	OpenLogs         func()
	Conversation     func(*ui.Context, *viewmodel.Conversation, *composer.State) (*composer.Context, ConversationActions)
	// DemoConversation lets the controller show over a demo conversation
	// what reads nothing but the conversation itself: the statistics
	// package's figures in the composer's context bar. It runs after the
	// demo's composer is prepared and sends the Node nothing of the demo.
	DemoConversation func(*ui.Context, *viewmodel.Conversation, *composer.State)
	RenderChild      func(*ui.Context, string)
	ReadSelection    func(string, string)
	ReaderActive     func(string) bool
	SelectPlugin     func(string) bool
	PluginPage       func(*ui.Context, string)
	PluginHeader     func(*ui.Context)
	PluginTitle      func(*ui.Context) bool
	PluginOverlays   func(*ui.Context)
	// Overview is a conversation's session overview for the header's button.
	Overview func(*ui.Context, *viewmodel.Conversation) *composer.Overview
}

type ConversationActions interface {
	composer.Actions
	conversation.Actions
}

// DropComposer forgets the input state of a draft that is gone and returns
// it, nil when there was none.
func (a *App) DropComposer(id string) *composer.State {
	state := a.drafts[id]
	delete(a.drafts, id)
	return state
}

// ReplaceConversationID retains local input state when a draft is created on Node.
func (a *App) ReplaceConversationID(before, after string) {
	if cv := a.Data.Find(before); cv != nil {
		cv.ID = after
	}
	if state := a.drafts[before]; state != nil {
		a.drafts[after] = state
		delete(a.drafts, before)
	}
	if a.Sidebar.Selected == before {
		a.Sidebar.Selected = after
	}
	a.renamePanel(before, after)
	if a.panes.Active == session(before) {
		a.panes.Active = session(after)
	}
	if a.panes.seen == before {
		a.panes.seen = after
	}
	if focus := a.panes.focus[session(before)]; focus != nil {
		a.panes.focus[session(after)] = focus
		delete(a.panes.focus, session(before))
	}
	var replace func(*panes.Node) *panes.Node
	replace = func(node *panes.Node) *panes.Node {
		if node == nil {
			return nil
		}
		copy := &panes.Node{Target: node.Target, Axis: node.Axis, Sizes: slices.Clone(node.Sizes)}
		if node.Target == session(before) {
			copy.Target = session(after)
		}
		for _, child := range node.Children {
			copy.Children = append(copy.Children, replace(child))
		}
		return copy
	}
	for i, group := range a.panes.Groups {
		a.panes.Groups[i] = replace(group)
	}
}

func (a *App) unavailable(c *ui.Context) {
	kit.Result(c, kit.ResultProps{Title: i18n.T(c, "unavailable")}).Grow(1).Justify(ui.Center)
}

func (a *App) notice(c *ui.Context) {
	if a.sampled() {
		kit.PreviewNotice(c)
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
					retained := false
					for _, pr := range d.Projects {
						if pr.ID == old.ID && pr.Host == old.Host {
							pr.Conversations = append([]*viewmodel.Conversation{cv}, pr.Conversations...)
							retained = true
							break
						}
					}
					if !retained {
						d.Unassigned = append([]*viewmodel.Conversation{cv}, d.Unassigned...)
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
	if a.Preview {
		a.hosts = nil
	}
}

// SetLocalHost supplies current local overview data without preview metrics.
func (a *App) SetLocalHost(h *host.Host) { a.hosts = map[string]*host.Host{h.ID: h} }

func (a *App) SetHosts(hosts map[string]*host.Host) { a.hosts = hosts }

func (a *App) SetHostPorts(id string, ports []host.Port) {
	if current := a.hosts[id]; current != nil {
		current.Ports = ports
	}
}

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
			f, known := a.lookupFeature(id)
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
