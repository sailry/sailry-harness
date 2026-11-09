// Package shell is the main window: the header band, the feature rail, and
// the panel holding the navigation beside the page in a resizable split
// (sailry-public apps/desktop/src/shell.rs, shell/frame.rs, shell/rail.rs).
package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/composer"
	"github.com/sailry/sailry-go/internal/conversation"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/settings"
	"github.com/sailry/sailry-go/internal/sidebar"
	"github.com/sailry/sailry-go/internal/theme"
)

// Shell metrics of preview.rs and sidebar.rs.
const (
	HeaderHeight = 48
	RailWidth    = 56
	NavWidth     = 240
	NavMin       = 200
	NavMax       = 320
	// trafficLights is the room macOS's window controls need at (12, 16).
	trafficLights = 80
)

// Page is what the main panel shows.
type Page int

const (
	ConversationPage Page = iota
	SettingsPage
)

// App is the window's state.
type App struct {
	Data     *preview.Data
	Mode     theme.Mode
	Page     Page
	NavOpen  bool
	NavWidth float32

	// OnMode, when set, gets the theme mode as it changes, so the window's
	// native material follows the app's appearance as theme.rs's
	// set_window_appearance does.
	OnMode  func(theme.Mode)
	applied theme.Mode

	Sidebar  *sidebar.State
	Settings *settings.State
	Messages *conversation.State
	drafts   map[string]*composer.State
}

// New makes the app over the preview dataset, showing its large conversation.
func New(d *preview.Data) *App {
	return &App{
		Data: d, NavOpen: true, NavWidth: NavWidth,
		Sidebar:  sidebar.New(d, preview.Big),
		Settings: settings.New(),
		Messages: conversation.New(),
		drafts:   map[string]*composer.State{},
	}
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
	theme.Apply(c, a.Mode)
	p := theme.Of(c)
	panel := p.Background
	if c.Vibrancy() && a.Mode == theme.System {
		// The window material shows through: the outer band at 45% of the
		// base, the panel at 83.5% (theme.rs surfaces). Only while the app
		// follows the system: a forced mode paints opaque, so the band never
		// shows a material of the other appearance.
		c.Root().Background(p.Background.Alpha(theme.SidebarOpacity))
		panel = p.Background.Alpha(theme.PanelOpacity)
	}
	ui.Column(c).Fill().Children(func() {
		a.header(c)
		ui.Row(c).Grow(1).AlignItems(ui.Stretch).Children(func() {
			a.rail(c)
			// The panel's outline: a 1 DIP top and left edge, rounded at the
			// top left (frame.rs).
			ui.Row(c.Key("panel")).Grow(1).AlignItems(ui.Stretch).BorderWidth(1, 0, 0, 1).BorderColor(p.SidebarBorder).
				Radius(8, 0, 0, 0).Clip().Children(func() {
				main := func() {
					ui.Column(c.Key("main")).Fill().Background(panel).Children(func() { a.page(c) })
				}
				if !a.NavOpen {
					main()
					return
				}
				nav := func() {
					ui.Column(c.Key("nav")).Fill().Background(p.Sidebar.Alpha(float32(panel.A) / 255)).Children(func() {
						if a.Page == SettingsPage {
							settings.Nav(c, a.Settings)
						} else {
							sidebar.View(c, a.Sidebar)
						}
					})
				}
				if ui.Split(c.Key("split"), &a.NavWidth, nav, main).SplitReset(NavWidth).Fill().Changed() {
					a.NavWidth = min(max(a.NavWidth, NavMin), NavMax)
				}
			})
		})
	})
}

// header is the band above the rail and the panel: the app's name over the
// navigation, clear of the traffic lights, then the page's header.
func (a *App) header(c *ui.Context) {
	p := theme.Of(c)
	bar := c.TitleBar()
	left := float32(RailWidth)
	if a.NavOpen {
		left += a.NavWidth
	}
	ui.Row(c.Key("header")).Height(HeaderHeight).Shrink(0).DragWindow().Children(func() {
		pad := float32(12)
		if bar.Left > 0 {
			pad = trafficLights
		}
		if a.NavOpen {
			ui.Row(c).Width(left).Height(HeaderHeight).PaddingStart(pad).PaddingEnd(12).Children(func() {
				ui.Text(c, i18n.T(c, "app")).FontSize(14).SingleLine().TextColor(p.Foreground)
			})
			pad = 12
		} else {
			pad = max(pad, left) - left + 12
		}
		ui.Row(c).Grow(1).Height(HeaderHeight).PaddingStart(pad).PaddingEnd(12 + bar.Right).Gap(8).Children(func() {
			icon := icons.PanelLeftClose
			if !a.NavOpen {
				icon = icons.PanelLeftOpen
			}
			theme.IconButton(c.Key("toggle"), icon, i18n.T(c, "sidebar_toggle"), 28, false).
				OnClick(func() { a.NavOpen = !a.NavOpen })
			ui.Text(c, a.title(c)).FontSize(14).SingleLine().Grow(1).Shrink(1).TextColor(p.Foreground)
			// Preview data is identified wherever it shows.
			// An opaque fill keeps it legible over the translucent band.
			ui.Row(c.Key("preview")).Radius(999).Background(p.Background).Tooltip(i18n.T(c, "preview_notice")).Children(func() {
				theme.Tag(c, i18n.T(c, "preview"))
			})
		})
	})
}

func (a *App) title(c *ui.Context) string {
	if a.Page == SettingsPage {
		return settings.Title(c, a.Settings)
	}
	if cv := a.Data.Find(a.Sidebar.Selected); cv != nil {
		return sidebar.Title(c, cv)
	}
	return i18n.T(c, "app")
}

// rail is the column of features (shell/rail.rs): 36 DIP ghost buttons, the
// conversation features at the top and search and settings at the bottom.
// Features this prototype does not have are shown disabled.
func (a *App) rail(c *ui.Context) {
	ui.Column(c.Key("rail")).Width(RailWidth).Shrink(0).Padding(8, 0).Gap(8).AlignItems(ui.Center).Children(func() {
		theme.IconButton(c.Key("conversations"), icons.Chat, i18n.T(c, "conversation"), 36, a.Page == ConversationPage).
			OnClick(func() { a.Page = ConversationPage })
		for _, f := range []struct {
			icon *ui.SVG
			key  string
		}{{icons.Grid, "activity"}, {icons.Puzzle, "market_title"}, {icons.Folder, "files"}, {icons.Network, "git"}} {
			theme.IconButton(c.Key(f.key), f.icon, i18n.T(c, f.key), 36, false).Disabled(true)
		}
		ui.Spacer(c)
		theme.IconButton(c.Key("more"), icons.Ellipsis, i18n.T(c, "more"), 36, false).Disabled(true)
		theme.IconButton(c.Key("search"), icons.Search, i18n.T(c, "search"), 36, false).Disabled(true)
		theme.IconButton(c.Key("settings"), icons.Settings, i18n.T(c, "settings"), 36, a.Page == SettingsPage).
			OnClick(func() { a.Page = SettingsPage })
	})
}

// page is the main panel: the conversation, or the settings page.
func (a *App) page(c *ui.Context) {
	if a.Page == SettingsPage {
		settings.Page(c, a.Settings, a.Data, &a.Mode)
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
