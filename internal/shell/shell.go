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

	Sidebar  *sidebar.State
	Settings *settings.State
	Messages *conversation.State
	drafts   map[string]*composer.State
	running  map[string]bool
}

// New makes the app over the preview dataset, showing its large conversation.
func New(d *preview.Data) *App {
	return &App{
		Data: d, NavOpen: true, NavWidth: NavWidth,
		Sidebar:  sidebar.New(d, preview.Big),
		Settings: settings.New(),
		Messages: conversation.New(),
		drafts:   map[string]*composer.State{},
		running:  map[string]bool{},
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
	theme.Apply(c, a.Mode)
	p := theme.Of(c)
	panel := p.Background
	if c.Vibrancy() {
		// The window material shows through: the outer band at 45% of the
		// base, the panel at 83.5% (theme.rs surfaces).
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
					ui.Column(c.Key("nav")).Fill().Background(p.Sidebar.Alpha(float32(panel.A)/255)).Children(func() {
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
			theme.Tag(c.Key("preview"), i18n.T(c, "preview")).Tooltip(i18n.T(c, "preview_notice"))
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
	id := cv.ID
	comp := a.Composer(id)
	comp.Running = a.running[id]
	conversation.View(c, a.Messages, cv, comp.Running)
	composer.View(c, comp, a.Data.Models, func(text string) { a.send(cv, text) }, func() { a.stop(cv) })
}

// send adds the message to the preview conversation and shows a reply
// running. Preview data calls no model: the reply only runs until Stop.
func (a *App) send(cv *preview.Conversation, text string) {
	n := len(cv.Messages)
	if n%2 == 1 { // a reply was stopped before it began: keep turns paired
		cv.Messages = append(cv.Messages, preview.Message{ID: n + 1, Role: preview.Assistant})
		n++
	}
	cv.Messages = append(cv.Messages, preview.Message{ID: n + 1, Role: preview.User, Text: text})
	cv.Updated, cv.Status = preview.Now, preview.Running
	a.running[cv.ID] = true
	a.Messages.List(cv.ID).ScrollToEnd()
}

func (a *App) stop(cv *preview.Conversation) {
	a.running[cv.ID] = false
	cv.Status = preview.Idle
}
