// Package shell is the main window's frame: the header band, the feature
// rail, the sidebar column and the right panel beside the page, each
// column opening, closing and resizing with the reference's motion, and
// the window's shortcuts, search, activity centre and knowledge viewer
// (sailry-public apps/desktop/src/shell.rs, shell/*.rs). See docs/shell.md.
package shell

import (
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/composer"
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/host"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/onboarding"
	"github.com/sailry/sailry-go/internal/panel"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/settings"
	"github.com/sailry/sailry-go/internal/sidebar"
	"github.com/sailry/sailry-go/internal/theme"
	"github.com/sailry/sailry-go/internal/transfers"
)

// Frame metrics of preview.rs, shell/limits.rs and shell/frame.rs.
const (
	HeaderHeight = 48
	RailWidth    = 56
	NavWidth     = 240
	NavMin       = 200
	NavMax       = 320
	MainMin      = 320 // the conversation's least width
	PanelWidth   = 560
	PanelMin     = 360
	// trafficLights is the room macOS's window controls need at (12, 16).
	trafficLights = 80
	// handle is the width of a column's resize handle, over its edge.
	handle = 6
)

// Motion is the columns' open and close (shell/frame/motion.rs): Kit's
// duration_normal along easing_move. MyGo makes it instant with Reduce
// Motion.
var Motion = ui.Animation{Duration: 180 * time.Millisecond, Ease: ui.CubicBezier(0.2, 0, 0, 1)}

// Page is what the main panel shows: the conversation, settings, or a
// rail feature's independent page (pages.go).
type Page int

const (
	ConversationPage Page = iota
	SettingsPage
	FilesPage
	GitPage
	ActivityPage
	MarketPage
	HostPage
)

// App is the window's state.
type App struct {
	Data *preview.Data
	// Theme is the selected theme package's ID; empty is the built-in one.
	Theme   string
	Mode    theme.Mode
	Effects bool // the theme's animated effects
	// Surfaces are the window materials' opacities (Appearance).
	Surfaces theme.Surfaces
	Page     Page
	NavOpen  bool
	NavWidth float32
	// PanelOpen is whether the user wants the right panel; a narrow window
	// hides it without changing this.
	PanelOpen  bool
	PanelWidth float32

	// OnMode, when set, gets the theme mode as it changes, so the window's
	// native material follows the app's appearance as theme.rs's
	// set_window_appearance does.
	OnMode  func(theme.Mode)
	applied theme.Mode

	Sidebar  *sidebar.State
	Settings *settings.State
	Messages *conversation.State
	Panel    *panel.State
	// Projects keeps each project's Files and Git state, shared by their
	// pages and the panel's tabs.
	Projects *panel.Projects
	drafts   map[string]*composer.State
	// transfers and voice play the composers' file transfers and voice
	// input on the frame clock (preview).
	transfers preview.Transfers
	voice     preview.Voice

	features []string        // the rail's order
	unpinned map[string]bool // features off the rail
	more     bool            // the rail's More popover
	search   search
	activity bool // the activity centre's popover
	reading  *reading
	shown    string // the conversation selected when a project page opened
	// The panes that take the focus: the main column, where the
	// conversation or a page shows, and the right panel (pane).
	mainPane, panelPane ui.Handle
	sent                []string // the native menu's commands (Do)
	// children are the subagent sessions opened in the panel, by ID.
	children map[string][]conversation.Turn

	// Host is the host chosen in the rail, whose page HostPage shows;
	// hosts are the host pages' data, made from the dataset as first
	// shown, and their UI state. Metrics is whether the host page's right
	// column shows the metrics (layout.panel_open of Page::Host).
	Host    string
	hosts   map[string]*host.Host
	hostUI  map[string]*host.State
	samples map[string]*sample
	Metrics bool

	// installs is the installation queue the rail's button shows;
	// download is the Save as dialog's transfer, played from downloaded
	// on the frame clock (preview).
	installs     *transfers.Queue
	installsOpen bool
	download     transfers.Download
	downloaded   time.Time

	// Welcome is the welcome guide, opened at the first launch
	// (SetPrefs) and from the Help menu in the preview.
	Welcome onboarding.Guide
	// selected is the selected text dialog (system shortcuts).
	selected selection
}

// New makes the app over the preview dataset, showing its large conversation.
func New(d *preview.Data) *App {
	a := &App{
		Data: d, NavOpen: true, NavWidth: NavWidth, PanelWidth: PanelWidth, Effects: true, Surfaces: theme.DefaultSurfaces,
		Sidebar:  sidebar.New(d, preview.Big),
		Settings: settings.New(),
		Messages: conversation.New(),
		Panel:    panel.New(d.Workspace),
		Projects: panel.NewProjects(d.Workspace),
		drafts:   map[string]*composer.State{},
		unpinned: map[string]bool{},
		Host:     "local",
		installs: preview.Installations(),
		Welcome:  onboarding.Guide{Seen: true},
		Metrics:  true,
	}
	for _, f := range Features {
		a.features = append(a.features, f.ID)
	}
	return a
}

// Composer returns the composer of a conversation, which keeps its draft.
func (a *App) Composer(id string) *composer.State {
	if s, ok := a.drafts[id]; ok {
		return s
	}
	s := composer.New(a.Data.Models[0].ID)
	s.Host = "local"
	if pr := a.Data.ProjectOf(id); pr != nil {
		s.Host, s.Project, s.Worktree = pr.Host, pr.ID, pr.Branch
		if cv := a.Data.Find(id); cv != nil && cv.Worktree != "" {
			s.Worktree = cv.Worktree
		}
	}
	a.drafts[id] = s
	return s
}

// View builds the window.
func (a *App) View(c *ui.Context) {
	if a.Mode != a.applied && a.OnMode != nil {
		a.OnMode(a.Mode)
	}
	a.applied = a.Mode
	theme.Apply(c, a.Theme, a.Mode)
	kit.SetEffects(a.Effects)
	a.shortcuts(c)
	a.followSelection()
	p := theme.Of(c)
	// The title bar and the rail are one outer band in title_bar's color
	// (theme.rs sidebar_background).
	band, panel := p.TitleBar, p.Background
	if c.Vibrancy() && a.Mode == theme.System {
		// The window material shows through: the outer band at 45% of the
		// title bar, the panel at 83.5% of the background (theme.rs
		// surfaces). Only while the app follows the system: a forced mode
		// paints opaque, so the band never shows a material of the other
		// appearance.
		band, panel = p.TitleBar.Alpha(a.Surfaces.Sidebar), p.Background.Alpha(a.Surfaces.Main)
	}
	c.Root().Background(band)
	// The window's art and effects lie under the band and the panel, which
	// turns translucent over them as over the window material.
	window := kit.Backdrop(c, "background.window")
	if window != nil {
		panel = p.Background.Alpha(a.Surfaces.Main)
	}
	root := kit.Draws(ui.Column(c.Key("frame")).Fill(), window)
	kit.ToastHost(c)
	f := a.frame(c, root)
	root.Children(func() {
		a.header(c, f)
		ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			a.rail(c)
			// The panel's outline: a 1 DIP top and left edge, rounded at the
			// top left (frame.rs, frame/corner.rs).
			ui.Row(c.Key("panel")).Grow(1).MinWidth(0).AlignItems(ui.Stretch).BorderWidth(1, 0, 0, 1).BorderColor(p.SidebarBorder).
				Radius(p.RadiusLg, 0, 0, 0).Clip().Children(func() {
				a.nav(c, f, panel)
				main := kit.Draws(ui.Column(c.Key("main")).Grow(1).MinWidth(0).Background(panel), kit.Backdrop(c, "background.content"))
				a.pane(c, main, &a.mainPane, a.closable(), func() { a.ClosePage() })
				main.Children(func() { a.page(c) })
				a.details(c, f, panel)
			})
		})
	})
	a.overlays(c)
}

// frame is where the columns are in this frame: how open each is, from 0
// to 1 along Motion, and their widths.
type frame struct {
	nav, details       float32 // openness
	navWidth, panelW   float32 // their full widths
	showsPanel, fitted bool
}

// frame animates the columns' openness on root, which stays from frame to
// frame.
func (a *App) frame(c *ui.Context, root ui.Element) frame {
	w, _ := c.Size()
	f := frame{navWidth: ClampNav(a.NavWidth)}
	navTarget := float32(0)
	if a.NavOpen {
		navTarget = 1
	}
	f.nav = root.AnimateTo("nav", navTarget, Motion)
	// Whether the panel fits is judged where the sidebar will end, so the
	// sidebar's motion does not toggle it (frame.rs).
	nav := f.navWidth * navTarget
	f.panelW = ClampPanel(a.PanelWidth, w, nav)
	f.fitted = PanelFits(w, nav, f.panelW)
	f.showsPanel = (a.Page == ConversationPage && a.PanelOpen || a.Page == HostPage && a.Metrics || a.extensionDetails()) && f.fitted
	target := float32(0)
	if f.showsPanel {
		target = 1
	}
	f.details = root.AnimateTo("details", target, Motion)
	return f
}

// nav is the sidebar's column: its full width clipped to how open it is,
// so its content slides rather than reflows, and the resize handle on its
// edge while it is fully open (frame.rs, frame/motion.rs).
func (a *App) nav(c *ui.Context, f frame, panel ui.Color) {
	if f.nav <= 0.001 {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("nav")).Width(f.navWidth * f.nav).Shrink(0).ClipX().AlignItems(ui.Stretch).Children(func() {
		col := ui.Column(c.Key("content")).Width(f.navWidth).Shrink(0).Background(p.Sidebar.Alpha(float32(panel.A)/255)).
			BorderWidth(0, 1, 0, 0).BorderColor(p.SidebarBorder)
		kit.Draws(col, kit.Backdrop(c, "background.sidebar")).Children(func() {
			if a.Page == SettingsPage {
				settings.Nav(c, a.Settings)
			} else {
				sidebar.View(c, a.Sidebar)
			}
		})
	})
	if f.nav >= 0.999 {
		resize(c, "nav-handle", i18n.T(c, "sidebar_toggle"), 1, &a.NavWidth, NavWidth, ClampNav)
	}
}

// details is the right panel's column, anchored at the window's right
// edge as it slides open and closed.
func (a *App) details(c *ui.Context, f frame, fill ui.Color) {
	if f.details <= 0.001 {
		return
	}
	p := theme.Of(c)
	if f.details >= 0.999 {
		w, _ := c.Size()
		resize(c, "panel-handle", i18n.T(c, "details"), -1, &a.PanelWidth, PanelWidth, func(v float32) float32 {
			return ClampPanel(v, w, f.navWidth*f.nav)
		})
	}
	ui.Row(c.Key("details")).Width(f.panelW * f.details).Shrink(0).ClipX().Justify(ui.End).AlignItems(ui.Stretch).Children(func() {
		col := ui.Column(c.Key("content")).Width(f.panelW).Shrink(0).Background(fill).BorderWidth(0, 0, 0, 1).BorderColor(p.Border)
		a.pane(c, col, &a.panelPane, true, func() {
			if a.extensionDetails() {
				a.Settings.CloseDetails()
				return
			}
			if a.Page == HostPage {
				a.Metrics = false
				a.mainPane.Focus(c)
				return
			}
			a.closeTab(c)
		})
		col.Children(func() {
			if a.extensionDetails() {
				settings.DetailsPanel(c, a.Settings)
				return
			}
			if a.Page == HostPage {
				h, st := a.hostPage()
				a.sample(c, h)
				host.MetricsPanel(c, st, h)
				return
			}
			a.Panel.Paged = a.paged()
			if pr := a.Data.ProjectOf(a.Sidebar.Selected); pr != nil {
				a.Panel.Cwd = pr.Path
			}
			panel.View(c, a.Panel, a.project(), kit.KeyText)
		})
	})
}

// resize is a column's drag handle: a 6 DIP strip over the edge with the
// resize cursor; dragging moves the edge by dir (1 for an edge on the
// right of the column, -1 on its left), and a double click puts the
// column back at reset. Custom: ui.Split keeps both panes 40 DIPs wide and
// cannot close a pane to nothing, which the columns' motion needs.
func resize(c *ui.Context, key, label string, dir float32, width *float32, reset float32, clamp func(float32) float32) {
	p := theme.Of(c)
	h := ui.Box(c.Key(key)).Width(handle).Margin(0, -handle/2).ZIndex(1).Cursor(ui.CursorResizeEW).Label(label).Shrink(0)
	if h.Hovered() || h.Pressed() {
		h.Background(p.Ring.Alpha(0.5))
	}
	if dx, _, ok := h.Dragged(); ok {
		*width = clamp(*width + dir*dx)
	}
	if h.DoubleClicked() {
		*width = clamp(reset)
	}
}

// togglePanel opens or closes the right panel (⌘⌥B, the header button);
// opened without a tab it shows the launcher.
func (a *App) togglePanel() {
	if a.Page == HostPage {
		a.Metrics = !a.Metrics
		return
	}
	a.PanelOpen = !a.PanelOpen
}

// openPanel shows a resource in the right panel.
func (a *App) openPanel(k panel.Kind) {
	a.Page, a.PanelOpen = ConversationPage, true
	a.Panel.Sync(a.project())
	a.Panel.Open(k)
}

// pane makes e a pane the focus goes into as it is clicked, as the
// reference's track_focus on a mouse down, and has it take ⌘W
// (CloseFocused) while the focus is inside it and it can close: the
// innermost element taking a key gets it (Element.Shortcut), so the panel
// and a page close only themselves, and nothing outside them sees the key.
func (a *App) pane(c *ui.Context, e ui.Element, ref *ui.Handle, closes bool, close func()) {
	e.Focusable().FocusRing(false).Bind(ref)
	if closes && e.Shortcut(CloseFocused.Mods, CloseFocused.Key) && !c.ShortcutRepeated() {
		close()
	}
}

// closeTab is ⌘W in the right panel (resources/close.rs): its active tab,
// the focus staying in the panel for the next, or the panel itself with
// none left, the focus then going back to the conversation.
func (a *App) closeTab(c *ui.Context) {
	if a.Panel.Active != (panel.Tab{}) && a.Panel.Close(a.Panel.Active) {
		a.panelPane.Focus(c)
		return
	}
	a.PanelOpen = false
	a.mainPane.Focus(c)
}

// adjacent is the conversation after or before the selected one in the
// sidebar's order.
func adjacent(a *App, forward bool) string {
	return sidebar.Adjacent(a.Sidebar.Order(), a.Sidebar.Selected, forward)
}

// page is the main panel: the conversation, the settings page, or a
// feature's page.
func (a *App) page(c *ui.Context) {
	switch a.Page {
	case SettingsPage:
		settings.Page(c, a.Settings, a.Data, settings.Look{Theme: &a.Theme, Mode: &a.Mode, Effects: &a.Effects, Surfaces: &a.Surfaces})
		return
	case MarketPage:
		settings.MarketPage(c, a.Settings)
		return
	case FilesPage, GitPage, ActivityPage:
		a.featurePage(c)
		return
	case HostPage:
		a.hostView(c)
		return
	}
	cv := a.Data.Find(a.Sidebar.Selected)
	if cv == nil {
		return
	}
	// The preview plays a sent reply on the frame clock, a step per Tick.
	preview.Advance(cv, c.Now())
	if cv.Playing() {
		c.After(preview.Tick)
	}
	comp := a.Composer(cv.ID)
	comp.Running = cv.Running()
	comp.Searching = a.Messages.SearchOpen()
	comp.Metrics = func(c *ui.Context) { conversation.UsageBar(c, cv.Session.Usage) }
	act := &actions{a: a, c: c, cv: cv, comp: comp}
	if a.transfers.Advance(comp.Attachments, c.Now()) {
		c.After(preview.Tick)
	}
	if changing, done := a.voice.Advance(&comp.Dictation, c.Now()); changing {
		c.After(preview.Tick)
	} else if done {
		c.Toast(i18n.T(c, "preview_notice"))
	}
	ctx := a.Data.Composer(cv, comp.Worktree)
	// Escape in the conversation after the picker and the search bar:
	// the queue's editor, then Stop (live/shortcuts.rs EscapeTarget).
	if comp.Escapes() && c.FocusWithin(a.mainPane) && c.Shortcut(0, ui.KeyEscape) {
		comp.Escape(act)
	}
	a.Panel.Child = func(c *ui.Context, id string) {
		conversation.View(c, a.Messages, id, a.children[id], nil, act)
	}
	if len(cv.Turns) == 0 {
		// A new session welcomes with the theme's hero and the mode
		// cards, whose prompts go to the composer as drafts.
		conversation.Welcome(c, a.Messages, func() {
			composer.View(c, comp, ctx, act)
		}, func(prompt string) { comp.Draft = prompt })
		return
	}
	conversation.View(c, a.Messages, cv.ID, cv.Turns, &cv.Session, act)
	composer.View(c, comp, ctx, act)
}

// actions carries out the page's actions on the preview data. Preview
// data calls no model or tool: decisions and answers change only the
// preview's rows, and say so in a toast.
type actions struct {
	a    *App
	c    *ui.Context
	cv   *preview.Conversation
	comp *composer.State
}

// Send plays the preview's reply; attachments and references go along
// in name only.
func (x *actions) Send(m composer.Message) {
	text := m.Text
	if text == "" && len(m.Attachments) > 0 {
		text = i18n.T(x.c, "chat_attachment_count", "count", len(m.Attachments))
	}
	preview.Send(x.cv, text, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) Queue(m composer.Message) {
	preview.Queue(x.cv, m, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) SendQueued(id string) {
	preview.SendQueued(x.cv, id, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) SaveQueued(id, text string)   { preview.SaveQueued(x.cv, id, text) }
func (x *actions) RemoveQueued(id string)       { preview.RemoveQueued(x.cv, id) }
func (x *actions) MoveQueued(id, before string) { preview.MoveQueued(x.cv, id, before) }

// Upload plays a transfer on the clock; the file is not read, which the
// toast says.
func (x *actions) Upload(a *composer.Attachment) {
	x.c.Toast(i18n.T(x.c, "preview_notice"))
	x.a.transfers.Start(a, x.c.Now())
	x.c.After(preview.Tick)
}

func (x *actions) Discard(a *composer.Attachment) { x.a.transfers.Drop(a) }

// Dictate plays voice input on the clock; no microphone opens.
func (x *actions) Dictate(d *composer.Dictation, cmd composer.DictationCommand) {
	x.a.voice.Command(d, cmd, x.c.Now())
	x.c.After(preview.Tick)
}

// Compact, a new worktree, a new project and the teams' settings would
// ask a model or write to disk: the preview says so.
func (x *actions) Compact()        { x.c.Toast(i18n.T(x.c, "preview_notice")) }
func (x *actions) CreateWorktree() { x.c.Toast(i18n.T(x.c, "preview_notice")) }
func (x *actions) AddProject()     { x.c.Toast(i18n.T(x.c, "preview_notice")) }
func (x *actions) ManageTeams()    { x.c.Toast(i18n.T(x.c, "preview_notice")) }

func (x *actions) Stop() { preview.Stop(x.cv) }

func (x *actions) Decide(id string, d composer.Decision, reason string) {
	preview.Decide(x.cv, id, d)
	x.c.Toast(i18n.T(x.c, "approval_preview_result"))
}

func (x *actions) Answer(id string, ans composer.Answer) {
	var answers []string
	if q := x.cv.Waiting.Question; q != nil {
		for _, i := range ans.Choices {
			answers = append(answers, q.Options[i])
		}
	}
	if ans.Text != "" {
		answers = append(answers, ans.Text)
	}
	preview.Resolve(x.cv, id, conversation.QuestionAccepted, answers)
	x.c.Toast(i18n.T(x.c, "preview_notice"))
}

func (x *actions) CancelQuestion(id string) {
	preview.Resolve(x.cv, id, conversation.QuestionCancelled, nil)
}

func (x *actions) DeclineQuestion(id string) {
	preview.Resolve(x.cv, id, conversation.QuestionDeclined, nil)
}

// OpenChild shows a subagent's session in a tab of the right panel: the
// preview's made-up run for it.
func (x *actions) OpenChild(id string) {
	title := ""
	for _, t := range x.cv.Turns {
		for _, b := range t.Reply.Blocks {
			for _, k := range b.Children {
				if k.ID == id {
					title = k.Title
				}
			}
		}
	}
	if x.a.children == nil {
		x.a.children = map[string][]conversation.Turn{}
	}
	if _, ok := x.a.children[id]; !ok {
		x.a.children[id] = preview.ChildTurns(id, title)
	}
	x.a.Page, x.a.PanelOpen = ConversationPage, true
	x.a.Panel.ShowChild(id, title)
}

// The transcript's actions that would run a model, change the session,
// write a file or leave the window only say so in the preview.
func (x *actions) notice() { x.c.Toast(i18n.T(x.c, "preview_notice")) }

func (x *actions) Retry(string)                             { x.notice() }
func (x *actions) Fork(string)                              { x.notice() }
func (x *actions) Rewind(string)                            { x.notice() }
func (x *actions) Worktree(string)                          { x.notice() }
func (x *actions) Export(string, conversation.ExportFormat) { x.notice() }
func (x *actions) Read(string, string)                      { x.notice() }
func (x *actions) Edit(string)                              { x.notice() }
func (x *actions) Undo(string, string)                      { x.notice() }
func (x *actions) Review(string, string)                    { x.notice() }
func (x *actions) Reconnect()                               { x.notice() }

// Quote adds the quoted selection to the end of the draft.
func (x *actions) Quote(text string) {
	comp := x.a.Composer(x.cv.ID)
	if comp.Draft != "" && !strings.HasSuffix(comp.Draft, "\n\n") {
		comp.Draft += "\n\n"
	}
	comp.Draft += text
}

// LoadOlder reveals the next page of the preview's older turns.
func (x *actions) LoadOlder() { preview.LoadOlder(x.cv) }

// FileAction opens a file in the system's app; saving and asking for
// edits only say so in the preview.
func (x *actions) FileAction(f conversation.File, a conversation.FileAction) {
	switch a {
	case conversation.OpenExternal:
		x.openSystem(f)
	case conversation.Download:
		x.a.startDownload(f.Name())
	default:
		x.notice()
	}
}

// openSystem hands a file to the system's default app through MyGo's
// OpenURL, the one place files leave the window. A file that is not
// there, as the preview's are not, toasts the failure.
func (x *actions) openSystem(f conversation.File) {
	path, err := filepath.Abs(f.Path)
	if err == nil {
		_, err = os.Stat(path)
	}
	if err != nil {
		kit.Toast(x.c, kit.Failed, i18n.T(x.c, "files_open_failed"))
		return
	}
	x.c.OpenURL((&url.URL{Scheme: "file", Path: path}).String())
}

// OpenFile shows a file's preview as a tab of the right panel; a file
// without a native preview opens in the system's app (WebView later).
func (x *actions) OpenFile(f conversation.File) {
	if f.Kind == conversation.OtherFile {
		x.openSystem(f)
		return
	}
	x.a.Page, x.a.PanelOpen = ConversationPage, true
	x.a.Panel.ShowArtifact(f)
}
