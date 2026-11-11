// Package shell is the main window's frame: the header band, the feature
// rail, the sidebar column and the right panel beside the page, each column
// opening, closing and resizing with the reference's motion, and the window's
// shortcuts, search, activity centre and knowledge viewer. See docs/shell.md.
package shell

import (
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/host"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/onboarding"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/transfers"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Layout metrics.
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

// The header's and the rail's icon buttons are Kit's IconButton at one size
// each, 8 DIPs apart. Header contributions (Hooks.PluginHeader) use
// HeaderButton too.
const (
	HeaderButton = 28
	RailButton   = 36
	ButtonGap    = 8
)

// Motion is the columns' open and close: Kit's duration_normal along
// easing_move. MyGo makes it instant with Reduce Motion.
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
	NotificationsPage
	ProjectPage
	PluginPage
)

// App is the window's state.
type App struct {
	Data         *viewmodel.Data
	Preview      bool
	Hooks        Hooks
	noticeTab    int
	noticeDetail *viewmodel.Notice
	// frosted is whether this frame's theme has window art, which the
	// columns blur (kit.Pane).
	frosted bool
	// Theme is the selected theme package's ID; empty is the built-in one.
	Theme   string
	Mode    theme.Mode
	Effects bool // the theme's animated effects
	// Surfaces are the window materials' opacities (Appearance).
	Surfaces theme.Surfaces
	Page     Page
	// NavOpen and NavWidth are the side column's saved choice, shared by the
	// pages that have one (HasNavigation); a page without one hides the
	// column and leaves the choice as it is.
	NavOpen  bool
	NavWidth float32
	// navigated is whether the last frame's page had a side column.
	navigated bool
	// PanelOpen is whether the user wants the right panel; a narrow window
	// hides it without changing this.
	PanelOpen  bool
	PanelWidth float32

	// OnMode, when set, gets the theme mode as it changes, so the window's
	// native material follows the app's appearance.
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

	features       []string // the rail's order
	pluginFeatures map[string]Feature
	pluginPage     string
	unpinned       map[string]bool // features off the rail
	// previewRail is the preview's rail in the shared UI state file, which a
	// live window writes back as it found it (Prefs).
	previewRail struct{ features, unpinned []string }
	// previewOrder is the preview's session order in that file, kept likewise.
	previewOrder string
	more         bool // the rail's More popover
	search       search
	activity     bool // the activity centre's popover
	reading      *reading
	shown        string // the conversation selected when a project page opened
	overview     *projectOverview
	// The panes that take the focus: the main column, where the
	// conversation or a page shows, and the right panel (pane).
	mainPane, panelPane ui.Handle
	sent                []string // the native menu's commands (Do)
	modal               bool     // a modal dialog of Kit's showed in the last frame
	fileSaveEnabled     bool
	fileSaveMenu        func(bool)
	menuKeys            []menuKey // the native menu's items with a key (syncMenuKeys)
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
	// closing is the close and quit prompt (closing.go).
	closing *closing
	// panes is the split workspace (panes.go).
	panes workspace
	// panels are the conversations' right panels (panels.go).
	panels conversationPanels
}

// New makes the app over the preview dataset, showing its large conversation.
func New(d *preview.Data) *App {
	a := newApp(d, preview.Big, settings.New())
	a.Preview = true
	a.installs = preview.Installations()
	return a
}

// NewLive creates an unseeded live presentation. It performs no backend I/O.
func NewLive(d *viewmodel.Data) *App {
	a := newApp(d, "", settings.NewLive())
	a.Panel.Live = true
	if len(d.Hosts) > 0 {
		a.Host = d.Hosts[0].ID
	}
	return a
}

func newApp(d *viewmodel.Data, selected string, st *settings.State) *App {
	a := &App{
		Data: d, NavOpen: true, NavWidth: NavWidth, PanelWidth: PanelWidth, Effects: true, Surfaces: theme.DefaultSurfaces,
		Sidebar: sidebar.New(d, selected), Settings: st, Messages: conversation.New(),
		Panel: panel.New(d.Workspace), Projects: panel.NewProjects(d.Workspace),
		drafts: map[string]*composer.State{}, unpinned: map[string]bool{}, Host: "local",
		Welcome: onboarding.Guide{Seen: true}, Metrics: true,
	}
	for _, f := range Features {
		a.features = append(a.features, f.ID)
	}
	// The sidebar's sort menu and the settings' session order are one choice.
	a.Sidebar.Sort = &st.Model.SessionOrder
	return a
}

// Composer returns the composer of a conversation, which keeps its draft.
func (a *App) Composer(id string) *composer.State {
	if s, ok := a.drafts[id]; ok {
		return s
	}
	model := ""
	if len(a.Data.Models) > 0 {
		model = a.Data.Models[0].ID
	}
	s := composer.New(model)
	s.Host = a.Host
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
	defer a.updateFileSaveMenu(c)
	kit.SetPreferences(c, kit.Preferences{ToastSeconds: uint64(settings.ToastSeconds[min(max(a.Settings.Toast, 0), len(settings.ToastSeconds)-1)]), MessageTextSize: uint8(a.Settings.TextSize)})
	// The panel is the selected conversation's before anything opens a tab
	// in it (panels.go).
	a.MountPanel()
	if a.Hooks.BeforeFrame != nil {
		a.Hooks.BeforeFrame(c)
	}
	if a.Hooks.AfterFrame != nil {
		defer a.Hooks.AfterFrame()
	}
	if a.Mode != a.applied && a.OnMode != nil {
		a.OnMode(a.Mode)
	}
	a.applied = a.Mode
	theme.Apply(c, a.Theme, a.Mode)
	kit.SetEffects(a.Effects)
	a.shortcuts(c)
	a.MountPanel()
	a.followSelection()
	a.followPanes()
	p := theme.Of(c)
	// The title bar and the rail are one outer band in title_bar's color.
	band, panel := p.TitleBar, p.Background
	if c.Vibrancy() && a.Mode == theme.System {
		// The window material shows through: the outer band at 45% of the title
		// bar, the panel at 83.5% of the background. Only while the app follows the
		// system: a forced mode paints opaque, so the band never shows a material
		// of the other appearance.
		band, panel = p.TitleBar.Alpha(a.Surfaces.Sidebar), p.Background.Alpha(a.Surfaces.Main)
	}
	c.Root().Background(band)
	// The window's art and effects lie under the band and the panel, which
	// turns translucent over them as over the window material.
	window := kit.Backdrop(c, "background.window")
	if window != nil {
		panel = p.Background.Alpha(a.Surfaces.Main)
	}
	a.frosted = window != nil
	root := kit.Draws(ui.Column(c.Key("frame")).Fill(), window)
	f := a.frame(c, root)
	root.Children(func() {
		a.header(c, f)
		ui.Row(c).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(func() {
			a.rail(c)
			// The panel's outline: a 1 DIP top and left edge, rounded at the top left.
			ui.Row(c.Key("panel")).Grow(1).MinWidth(0).AlignItems(ui.Stretch).BorderWidth(1, 0, 0, 1).BorderColor(p.SidebarBorder).
				Radius(p.RadiusLg, 0, 0, 0).Clip().Children(func() {
				a.nav(c, f, panel)
				main := kit.Draws(kit.Pane(c, ui.Column(c.Key("main")).Grow(1).MinWidth(0), panel, a.frosted, a.Surfaces.Blur*theme.PaneBlurMax), kit.Backdrop(c, "background.content"))
				a.pane(c, main, &a.mainPane, a.closable(), func() { a.ClosePage() })
				a.mainColumn(c, main)
				a.details(c, f, panel)
			})
		})
	})
	a.overlays(c)
	if a.Hooks.PluginOverlays != nil {
		a.Hooks.PluginOverlays(c)
	}
	// Last, over every dialog: a toast shown while one is open stays reachable.
	kit.ToastHost(c)
}

// frame is where the columns are in this frame: how open each is, from 0
// to 1 along Motion, and their widths.
type frame struct {
	nav, details       float32 // openness
	navWidth, panelW   float32 // their full widths
	showsPanel, fitted bool
}

// HasNavigation reports whether the page shown has a side column beside the
// rail, as the reference's feature_navigation: the sessions on the
// conversation and on the host and project pages opened from it, the
// sections on Settings. Activity, Files, Git, Market, the notification
// center and a plugin's page have none: a plugin lays out its own columns
// inside its page.
func (a *App) HasNavigation() bool {
	switch a.Page {
	case ConversationPage, HostPage, ProjectPage, SettingsPage:
		return true
	}
	return false
}

// NavShown reports whether the side column shows: the page has one and the
// user keeps it open.
func (a *App) NavShown() bool { return a.NavOpen && a.HasNavigation() }

// toggleNav opens or closes the side column (⌘B, the header button, the
// menu). A page without one keeps the saved choice, as the reference's
// toggle_sidebar returns when no navigation is available.
func (a *App) toggleNav() {
	if a.HasNavigation() {
		a.NavOpen = !a.NavOpen
	}
}

// frame animates the columns' openness on root, which stays from frame to
// frame.
func (a *App) frame(c *ui.Context, root ui.Element) frame {
	w, _ := c.Size()
	f := frame{navWidth: ClampNav(a.NavWidth)}
	navTarget := float32(0)
	if a.NavShown() {
		navTarget = 1
	}
	if navigated := a.HasNavigation(); navigated && a.navigated {
		f.nav = root.AnimateTo("nav", navTarget, Motion)
	} else {
		// The column belongs to the page: going to or from a page without
		// one shows the destination settled, with no motion.
		f.nav = root.Jump("nav", navTarget)
		a.navigated = navigated
	}
	// Whether the panel fits is judged where the sidebar will end, so the
	// sidebar's motion does not toggle it.
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

// nav is the sidebar's column: its full width clipped to how open it is, so
// its content slides rather than reflows, and the resize handle on its edge
// while it is fully open.
func (a *App) nav(c *ui.Context, f frame, panel ui.Color) {
	if f.nav <= 0.001 {
		return
	}
	p := theme.Of(c)
	ui.Row(c.Key("nav")).Width(f.navWidth * f.nav).Shrink(0).ClipX().AlignItems(ui.Stretch).Children(func() {
		col := kit.Pane(c, ui.Column(c.Key("content")).Width(f.navWidth).Shrink(0), p.Sidebar.Alpha(float32(panel.A)/255), a.frosted, a.Surfaces.Blur*theme.PaneBlurMax).
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
		col := kit.Pane(c, ui.Column(c.Key("content")).Width(f.panelW).Shrink(0), fill, a.frosted, a.Surfaces.Blur*theme.PaneBlurMax).BorderWidth(0, 0, 0, 1).BorderColor(p.Border)
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
			a.syncPanel()
			panel.View(c, a.Panel, a.project(), kit.KeyText)
		})
	})
}

// syncPanel gives the panel what the page shows beside it, before its tabs
// in the header and its content in the column are built.
func (a *App) syncPanel() {
	a.Panel.Paged = a.paged()
	a.Panel.HasProject = a.Data.ProjectOf(a.Sidebar.Selected) != nil
	a.Panel.Cwd = ""
	if pr := a.Data.ProjectOf(a.Sidebar.Selected); pr != nil {
		a.Panel.Cwd = pr.Path
	}
	if cv := a.Data.Find(a.Sidebar.Selected); cv != nil && cv.WorktreePath != "" {
		a.Panel.Cwd = cv.WorktreePath
	}
	a.Panel.Focus = func(c *ui.Context) { a.panelPane.Focus(c) }
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
		// A 2 DIP line in the middle of the wider hit area.
		h.Justify(ui.Center).Children(func() { ui.Box(c).Width(2).FillHeight().Background(p.Ring.Alpha(0.5)) })
	}
	if dx, _, ok := h.Dragged(); ok {
		*width = clamp(*width + dir*dx)
	}
	if h.DoubleClicked() {
		*width = clamp(reset)
	}
}

// togglePanel opens or closes the right panel (⌘⌥B, the header button);
// opened without a tab it shows the launcher. It is the right column's only
// close control.
func (a *App) togglePanel() {
	if a.extensionDetails() {
		// The details show for a selected item: folding them deselects it.
		a.Settings.CloseDetails()
		return
	}
	if a.Page == HostPage {
		a.Metrics = !a.Metrics
		return
	}
	a.PanelOpen = !a.PanelOpen
}

// openPanel shows a resource in the right panel.
func (a *App) openPanel(k panel.Kind) {
	a.MountPanel()
	a.syncPanel()
	if !a.Panel.Offers(k) {
		// A shortcut opens only what the launcher lists.
		return
	}
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

// closeTab is ⌘W in the right panel: its active tab, the focus staying in the
// panel for the next, or the panel itself with none left, the focus then
// going back to the conversation.
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
	case NotificationsPage:
		a.notificationCenter(c)
		return
	case SettingsPage:
		settings.Page(c, a.Settings, a.Data, settings.Look{Theme: &a.Theme, Mode: &a.Mode, Effects: &a.Effects, Surfaces: &a.Surfaces})
		return
	case MarketPage:
		settings.MarketPage(c, a.Settings)
		return
	case FilesPage, GitPage, ActivityPage, PluginPage:
		a.featurePage(c)
		return
	case HostPage:
		a.hostView(c)
		return
	case ProjectPage:
		a.projectView(c)
		return
	}
	if a.split(c) {
		return
	}
	if cv := a.Data.Find(a.Sidebar.Selected); cv != nil {
		a.conversation(c, cv, a.mainPane)
	}
}

// conversation is a session's transcript and composer, alone in the main
// column or in a pane of a split, focus being where it takes its keys.
// Each session keeps its own list, composer and search: the transcript's
// state is kept by session, and a session shows in one pane at most.
func (a *App) conversation(c *ui.Context, cv *preview.Conversation, focus ui.Handle) {
	// Only the explicit preview advances simulated replies.
	if a.Preview {
		preview.Advance(cv, c.Now())
	}
	if a.Preview && cv.Playing() {
		c.After(preview.Tick)
	}
	comp := a.Composer(cv.ID)
	comp.Running = cv.Running()
	comp.Searching = a.Messages.SearchOpen() && cv.ID == a.Sidebar.Selected
	comp.Metrics = func(c *ui.Context, compact bool, open *bool) {
		conversation.UsageBar(c, cv.Session.Usage, compact, open)
	}
	// The preview draws its own ring. A live session's is a package's
	// indicator, which its context carries: both would be two rings.
	comp.Indicator = nil
	if a.Preview {
		comp.Indicator = func(c *ui.Context) { conversation.ContextRing(c, cv.Session.Usage) }
	}
	var act ConversationActions = &actions{a: a, c: c, cv: cv, comp: comp}
	if a.Preview && a.transfers.Advance(comp.Attachments, c.Now()) {
		c.After(preview.Tick)
	}
	if a.Preview {
		if changing, done := a.voice.Advance(&comp.Dictation, c.Now()); changing {
			c.After(preview.Tick)
		} else if done {
			kit.Message(c, i18n.T(c, "preview_notice"))
		}
	}
	ctx := &composer.Context{Models: a.Data.Models, Readonly: true}
	if a.Preview {
		ctx = preview.Composer(a.Data, cv, comp.Worktree)
	} else if a.Hooks.Conversation != nil {
		ctx, act = a.Hooks.Conversation(c, cv, comp)
	}
	// Escape in the conversation after the picker and the search bar: the
	// queue's editor, then Stop.
	if comp.Escapes() && c.FocusWithin(focus) && c.Shortcut(0, ui.KeyEscape) {
		comp.Escape(act)
	}
	if cv.ID == a.Sidebar.Selected {
		if !a.Preview && a.Hooks.RenderChild != nil {
			a.Panel.Child = a.Hooks.RenderChild
		} else {
			a.Panel.Child = func(c *ui.Context, id string) {
				conversation.View(c, a.Messages, id, a.children[id], nil, act)
			}
		}
	}
	if len(cv.Turns) == 0 && !cv.Session.Loading && cv.Session.Connection == conversation.Connected {
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
	if x.blocked() {
		return
	}
	text := m.Text
	if text == "" && len(m.Attachments) > 0 {
		text = i18n.T(x.c, "chat_attachment_count", "count", len(m.Attachments))
	}
	preview.Send(x.cv, text, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

// Guide joins the message to the running reply, pending in the queue.
func (x *actions) Guide(m composer.Message) {
	if x.blocked() {
		return
	}
	preview.Guide(x.cv, m, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) Queue(m composer.Message) {
	if x.blocked() {
		return
	}
	preview.Queue(x.cv, m, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) SendQueued(id string) {
	if x.blocked() {
		return
	}
	preview.SendQueued(x.cv, id, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

func (x *actions) WithdrawGuidance(id string) {
	if x.blocked() {
		return
	}
	preview.WithdrawGuidance(x.cv, id)
}
func (x *actions) SaveQueued(id, text string) {
	if x.blocked() {
		return
	}
	preview.SaveQueued(x.cv, id, text)
}
func (x *actions) RemoveQueued(id string) {
	if x.blocked() {
		return
	}
	preview.RemoveQueued(x.cv, id)
}
func (x *actions) MoveQueued(id, before string) {
	if x.blocked() {
		return
	}
	preview.MoveQueued(x.cv, id, before)
}

// Upload plays a transfer on the clock; the file is not read, which the
// toast says.
func (x *actions) Upload(a *composer.Attachment) {
	if x.blocked() {
		return
	}
	x.notice()
	x.a.transfers.Start(a, x.c.Now())
	x.c.After(preview.Tick)
}

func (x *actions) Discard(a *composer.Attachment) {
	if x.blocked() {
		return
	}
	x.a.transfers.Drop(a)
}

// Dictate plays voice input on the clock; no microphone opens.
func (x *actions) Dictate(d *composer.Dictation, cmd composer.DictationCommand) {
	if x.blocked() {
		return
	}
	x.a.voice.Command(d, cmd, x.c.Now())
	x.c.After(preview.Tick)
}

// Compact, a new worktree, a new project and the teams' settings would
// ask a model or write to disk: the preview says so.
func (x *actions) Compact()        { x.notice() }
func (x *actions) CreateWorktree() { x.notice() }
func (x *actions) AddProject()     { x.notice() }
func (x *actions) ManageTeams()    { x.notice() }

func (x *actions) Stop() {
	if x.blocked() {
		return
	}
	preview.Stop(x.cv)
}

// The activity strip: a file of the Git badge opens its diff; a plugin's
// action, stopping a command and opening a port would reach the host, so
// the preview says so.
func (x *actions) OpenChange(path string) {
	x.a.openPanel(panel.Changes)
	x.a.Panel.Show(panel.Tab{Kind: panel.Changes, ID: path})
}
func (x *actions) Badge(_, _ string)  { x.notice() }
func (x *actions) StopCommand(string) { x.notice() }
func (x *actions) OpenService(int)    { x.notice() }

func (x *actions) Decide(id string, d composer.Decision, reason string) {
	if x.blocked() {
		return
	}
	preview.Decide(x.cv, id, d)
	kit.Message(x.c, i18n.T(x.c, "approval_preview_result"))
}

func (x *actions) Answer(id string, ans composer.Answer) {
	if x.blocked() {
		return
	}
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
	x.notice()
}

func (x *actions) CancelQuestion(id string) {
	if x.blocked() {
		return
	}
	preview.Resolve(x.cv, id, conversation.QuestionCancelled, nil)
}

func (x *actions) DeclineQuestion(id string) {
	if x.blocked() {
		return
	}
	preview.Resolve(x.cv, id, conversation.QuestionDeclined, nil)
}

// OpenChild shows a subagent's session in a tab of the right panel: the
// preview's made-up run for it.
func (x *actions) OpenChild(id string) {
	if x.blocked() {
		return
	}
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
func (x *actions) notice() {
	if x.a.Preview {
		kit.Message(x.c, i18n.T(x.c, "preview_notice"))
		return
	}
	kit.Notify(x.c, "", i18n.T(x.c, "unavailable"))
}

func (x *actions) blocked() bool {
	if !x.a.Preview {
		x.notice()
		return true
	}
	return false
}

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

// ImageAction is the image viewer's Copy, Save or Open, which would write
// the clipboard, a file or leave the window: the preview only says so.
func (x *actions) ImageAction(kit.ImageAction, conversation.Image) { x.notice() }

// Quote adds the quoted selection to the end of the draft.
func (x *actions) Quote(text string) {
	comp := x.a.Composer(x.cv.ID)
	if comp.Draft != "" && !strings.HasSuffix(comp.Draft, "\n\n") {
		comp.Draft += "\n\n"
	}
	comp.Draft += text
}

// LoadOlder reveals the next page of the preview's older turns.
func (x *actions) LoadOlder() {
	if x.blocked() {
		return
	}
	preview.LoadOlder(x.cv)
}

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
