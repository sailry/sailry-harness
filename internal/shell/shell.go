// Package shell is the main window's frame: the header band, the feature
// rail, the sidebar column and the right panel beside the page, each
// column opening, closing and resizing with the reference's motion, and
// the window's shortcuts, search, activity centre and knowledge viewer
// (sailry-public apps/desktop/src/shell.rs, shell/*.rs). See docs/shell.md.
package shell

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/composer"
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/panel"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/settings"
	"github.com/sailry/sailry-go/internal/sidebar"
	"github.com/sailry/sailry-go/internal/theme"
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
)

// App is the window's state.
type App struct {
	Data *preview.Data
	// Theme is the selected theme package's ID; empty is the built-in one.
	Theme    string
	Mode     theme.Mode
	Effects  bool // the theme's animated effects
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
}

// New makes the app over the preview dataset, showing its large conversation.
func New(d *preview.Data) *App {
	a := &App{
		Data: d, NavOpen: true, NavWidth: NavWidth, PanelWidth: PanelWidth, Effects: true,
		Sidebar:  sidebar.New(d, preview.Big),
		Settings: settings.New(),
		Messages: conversation.New(),
		Panel:    panel.New(d.Workspace),
		Projects: panel.NewProjects(d.Workspace),
		drafts:   map[string]*composer.State{},
		unpinned: map[string]bool{},
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
		band, panel = p.TitleBar.Alpha(theme.SidebarOpacity), p.Background.Alpha(theme.PanelOpacity)
	}
	c.Root().Background(band)
	// The window's art and effects lie under the band and the panel, which
	// turns translucent over them as over the window material.
	window := kit.Backdrop(c, "background.window")
	if window != nil {
		panel = p.Background.Alpha(theme.PanelOpacity)
	}
	root := kit.Draws(ui.Column(c.Key("frame")).Fill(), window)
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
	kit.Toasts(c)
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
	f.showsPanel = a.Page == ConversationPage && a.PanelOpen && f.fitted
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
		a.pane(c, col, &a.panelPane, true, func() { a.closeTab(c) })
		col.Children(func() {
			a.Panel.Paged = a.paged()
			panel.View(c, a.Panel, a.project(), KeyText)
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
	a.PanelOpen = !a.PanelOpen
}

// openPanel shows a resource in the right panel.
func (a *App) openPanel(k panel.Kind) {
	a.Page, a.PanelOpen = ConversationPage, true
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
// the panel itself with none left, the focus then going back to the
// conversation.
func (a *App) closeTab(c *ui.Context) {
	if a.Panel.Active != "" && a.Panel.Close(a.Panel.Active) {
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
		settings.Page(c, a.Settings, a.Data, settings.Look{Theme: &a.Theme, Mode: &a.Mode, Effects: &a.Effects})
		return
	case FilesPage, GitPage, ActivityPage, MarketPage:
		a.featurePage(c)
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
	act := &actions{a: a, c: c, cv: cv}
	if len(cv.Turns) == 0 {
		// A new session welcomes with the theme's hero and the mode
		// cards, whose prompts go to the composer as drafts.
		conversation.Welcome(c, a.Messages, func() {
			composer.View(c, comp, a.Data.Models, cv.Waiting, act)
		}, func(prompt string) { comp.Draft = prompt })
		return
	}
	conversation.View(c, a.Messages, cv.ID, cv.Turns, act)
	composer.View(c, comp, a.Data.Models, cv.Waiting, act)
}

// actions carries out the page's actions on the preview data. Preview
// data calls no model or tool: decisions and answers change only the
// preview's rows, and say so in a toast.
type actions struct {
	a  *App
	c  *ui.Context
	cv *preview.Conversation
}

func (x *actions) Send(text string) {
	preview.Send(x.cv, text, x.c.Now())
	x.a.Messages.List(x.cv.ID).ScrollToEnd()
}

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

// OpenChild has no child session to open in the preview.
func (x *actions) OpenChild(string) { x.c.Toast(i18n.T(x.c, "preview_notice")) }

// Retry does not run a turn again in the preview.
func (x *actions) Retry(string) { x.c.Toast(i18n.T(x.c, "preview_notice")) }
