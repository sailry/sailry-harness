package shell

import (
	"slices"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Demo mode (Settings, Experimental) shows the demo conversations in place of
// the window's own, through the same sidebar, transcript, composer and panel.
// The window keeps two stages, the conversations and everything that belongs
// to them: its own and the demo's. One is on the App's fields, where the
// views read it, and the other waits in demoMode. The window's own stage is
// back on the fields while the controller's frame hooks run and while the
// window state is saved, so the controller projects, prunes and saves what
// it always does and never sees the demo. The demo's actions are the
// preview's: they change the demo's memory and reach no Node.

// stage is the conversations a window shows and the state that belongs to
// them.
type stage struct {
	data       *viewmodel.Data
	sidebar    *sidebar.State
	messages   *conversation.State
	panel      *panel.State
	projects   *panel.Projects
	drafts     map[string]*composer.State
	children   map[string][]conversation.Turn
	panes      workspace
	panels     conversationPanels
	panelOpen  bool
	panelWidth float32
	shown      string
	overview   *projectOverview
}

// demoMode is the demo while it shows: the stage that waits, the language
// the demo was written in, and the usage the demo counts.
type demoMode struct {
	parked stage
	locale string
	usage  *preview.Usage
	// under is set while the window's own stage is back on the fields
	// (underneath).
	under bool
	// data is the demo's dataset, which DemoConversation reads, and messages
	// its transcripts' state, which DemoReveal scrolls.
	data     *viewmodel.Data
	messages *conversation.State
}

func (a *App) onStage() stage {
	return stage{data: a.Data, sidebar: a.Sidebar, messages: a.Messages, panel: a.Panel, projects: a.Projects,
		drafts: a.drafts, children: a.children, panes: a.panes, panels: a.panels, panelOpen: a.PanelOpen,
		panelWidth: a.PanelWidth, shown: a.shown, overview: a.overview}
}

func (a *App) setStage(s stage) {
	a.Data, a.Sidebar, a.Messages, a.Panel, a.Projects = s.data, s.sidebar, s.messages, s.panel, s.projects
	a.drafts, a.children, a.panes, a.panels = s.drafts, s.children, s.panes, s.panels
	a.PanelOpen, a.PanelWidth, a.shown, a.overview = s.panelOpen, s.panelWidth, s.shown, s.overview
}

// Demo reports whether the window shows the demo conversations.
func (a *App) Demo() bool { return a.demo != nil && !a.demo.under }

// DemoConversation is the demo's conversation of that ID, nil without the
// demo. Unlike Demo it also answers while the window's own stage is back on
// the fields: the controller's frame hooks keep what it mounted over a demo
// conversation (Hooks.DemoConversation) for as long as the demo has it.
func (a *App) DemoConversation(id string) *viewmodel.Conversation {
	if a.demo == nil || a.demo.data == nil {
		return nil
	}
	return a.demo.data.Find(id)
}

// DemoReveal scrolls the demo's transcript of that conversation to the turn
// at index; nothing without the demo. Like DemoConversation it also answers
// while the window's own stage is back on the fields, where a package
// mounted over a demo conversation asks for it.
func (a *App) DemoReveal(id string, index int) {
	if a.demo == nil || a.demo.messages == nil {
		return
	}
	a.demo.messages.List(id).ScrollTo(index, ui.Center)
}

// DemoUsage is the usage the demo counts, which the Usage page reads while
// the demo shows; nil without the demo. Unlike Demo it also answers while
// the window's own stage is back on the fields.
func (a *App) DemoUsage() *preview.Usage {
	if a.demo == nil {
		return nil
	}
	return a.demo.usage
}

// sampled reports whether the conversations shown are sample data, whose
// actions stay in memory: the preview's, or the demo's.
func (a *App) sampled() bool { return a.Preview || a.Demo() }

// syncDemo follows the Demo mode setting at the start of a frame: it puts
// the demo on the window or takes it off, and writes the demo again in the
// window's language when that changed.
func (a *App) syncDemo(c *ui.Context) {
	on, locale := a.Settings.Model.DemoMode, i18n.Match(c.Locale())
	changed := false
	if a.demo != nil && (!on || a.demo.locale != locale) {
		a.setStage(a.demo.parked)
		a.demo, changed = nil, true
	}
	if on && a.demo == nil {
		a.enterDemo(locale)
		changed = true
	}
	kit.SetDemo(c, a.demo != nil)
	if changed && a.Hooks.DemoChanged != nil {
		a.underneath(a.Hooks.DemoChanged)
	}
	if a.demo == nil {
		return
	}
	// What is the window's rather than the conversations' stays its own: the
	// hosts, the performance indicator and the message display.
	own := a.demo.parked
	a.Data.Hosts = own.data.Hosts
	a.Sidebar.Metrics = own.sidebar.Metrics
	a.Messages.CompactDisplay = own.messages.CompactDisplay
	// A conversation made in the demo answers as the others do.
	preview.DemoReplies(a.Data, a.demo.locale)
}

// enterDemo parks the window's stage and shows a new demo.
func (a *App) enterDemo(locale string) {
	own := a.onStage()
	host := a.Host
	for _, h := range own.data.Hosts {
		if h.Local {
			host = h.ID
			break
		}
	}
	d := preview.Demo(locale, host)
	a.demo = &demoMode{parked: own, locale: locale, usage: preview.DemoUsage(d, locale, time.Now)}
	side := sidebar.New(d, preview.DemoStart)
	side.Sort = own.sidebar.Sort
	side.Sample = sidebar.Sample{Overview: a.demoProject, NewTerminal: a.demoTerminal}
	shown := panel.New(d.Workspace)
	shown.Demo = true
	a.demo.data = d
	// A reply's links to the demo's files and pictures show as their cards.
	messages := conversation.New()
	messages.Embed = func(session, dest, _ string, image bool) (conversation.Embed, bool) {
		return preview.Embed(d.Find(session), dest, image)
	}
	a.demo.messages = messages
	a.setStage(stage{data: d, sidebar: side, messages: messages, panel: shown,
		projects: panel.NewProjects(d.Workspace), drafts: map[string]*composer.State{}, panelWidth: own.panelWidth})
	for id, draft := range preview.DemoDrafts(locale) {
		comp := a.Composer(id)
		comp.Draft, comp.Attachments = draft.Text, draft.Attachments
	}
}

// demoProject shows a demo project's page, as the live window's Project
// overview does: its sessions open, New session and New terminal act on the
// demo, and its Terminal records are the terminal tabs open beside the
// conversation. Nothing reaches a Node.
func (a *App) demoProject(pr *viewmodel.Project) {
	a.OpenProject(pr, ProjectActions{
		NewSession: func(pr *viewmodel.Project) { a.Sidebar.NewSession(pr) },
		OpenSession: func(cv *viewmodel.Conversation, done func(bool)) {
			a.Page, a.Sidebar.Selected = ConversationPage, cv.ID
			done(true)
		},
		NewTerminal: a.demoTerminal,
		Terminals: func() []ProjectTerminal {
			var list []ProjectTerminal
			if a.Data.ProjectOf(a.Sidebar.Selected) != pr {
				return list
			}
			for _, t := range a.Panel.Tabs {
				if t.Kind == panel.Terminal {
					list = append(list, ProjectTerminal{ID: t.ID, Path: pr.Path})
				}
			}
			return list
		},
		OpenTerminal: func(id string) {
			a.Page, a.PanelOpen = ConversationPage, true
			a.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: id})
		},
	})
}

// demoTerminal opens a terminal tab in a demo project: beside the
// conversation shown when it is the project's, else beside the project's
// first, or a new one. The tab shows the demo's screen; no shell starts.
func (a *App) demoTerminal(pr *viewmodel.Project) {
	if a.Data.ProjectOf(a.Sidebar.Selected) != pr {
		at := slices.IndexFunc(pr.Conversations, func(cv *viewmodel.Conversation) bool { return !cv.Archived })
		if at < 0 {
			a.Sidebar.NewSession(pr)
		} else {
			a.Sidebar.Selected = pr.Conversations[at].ID
		}
	}
	a.openPanel(panel.Terminal)
}

// underneath runs f over the window's own stage, the demo waiting meanwhile:
// the controller's frame hooks and the saved window state.
func (a *App) underneath(f func()) {
	if a.demo == nil || a.demo.under {
		f()
		return
	}
	demo := a.onStage()
	a.setStage(a.demo.parked)
	a.demo.under = true
	defer func() {
		a.demo.under = false
		a.demo.parked = a.onStage()
		a.setStage(demo)
	}()
	f()
}

// Prefs returns the app's preferences as they are now. They are the window's
// own: the demo's panels and panes are never saved.
func (a *App) Prefs() Prefs {
	var p Prefs
	a.underneath(func() { p = a.prefs() })
	return p
}

// sampleComposer is what the composer of a sample conversation is offered:
// the preview's, or the demo's with its own models.
func (a *App) sampleComposer(cv *viewmodel.Conversation, comp *composer.State) *composer.Context {
	if a.Demo() {
		return preview.DemoComposer(a.Data, cv, comp.Worktree)
	}
	return preview.Composer(a.Data, cv, comp.Worktree)
}
