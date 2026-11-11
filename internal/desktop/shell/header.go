package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// header is the band above the rail and the columns: the app's name over the
// sidebar, clear of the traffic lights, then two regions that meet at the
// right panel's left edge. The main region is the page's: the sidebar toggle,
// the title and, at its end, the page's own actions. The region over the
// panel is the panel's: its tabs or its title. The session status and the
// fold toggle are fixed at the window's far right, clear of the window
// buttons, in both states. Its empty areas drag the window.
func (a *App) header(c *ui.Context, f frame) {
	p := theme.Of(c)
	bar := c.TitleBar()
	left := RailWidth + f.navWidth*f.nav
	ui.Row(c.Key("header")).Height(HeaderHeight).Shrink(0).DragWindow().Children(func() {
		// The traffic lights' room, from MyGo's title bar: none in full screen or
		// without lights. The brand column keeps the rail's width while the sidebar
		// is collapsed, so the page's header starts past the lights through the
		// motion.
		lights := float32(0)
		if bar.Left > 0 {
			lights = max(bar.Left, trafficLights)
		}
		ui.Row(c.Key("brand")).Width(left).Height(HeaderHeight).PaddingStart(max(lights, 12)).PaddingEnd(12).ClipX().Children(func() {
			if f.nav > 0.001 {
				ui.Text(c, i18n.T(c, "app")).FontSize(14).SingleLine().TextColor(p.SidebarForeground).Opacity(f.nav)
				// A short divider over the sidebar's edge, apart from the
				// edge's own line below the band.
				ui.Box(c.Key("divider")).Absolute().Right(0).Top((HeaderHeight-dividerHeight)/2).Size(1, dividerHeight).
					Background(p.TitleBarBorder).Opacity(f.nav)
			}
		})
		pad := max(lights-left, 0) + 12
		// The main region ends one gap before the fixed cluster, and 12 before
		// the split while the panel shows.
		ui.Row(c.Key("module")).Grow(1).MinWidth(0).Height(HeaderHeight).PaddingStart(pad).PaddingEnd(ButtonGap + 4*f.details).Gap(ButtonGap).Children(func() {
			// The toggle belongs to the side column: a page without one shows
			// none, as the reference's module header.
			if a.HasNavigation() {
				icon := icons.PanelLeftClose
				if !a.NavOpen {
					icon = icons.PanelLeftOpen
				}
				kit.IconButtonHint(c.Key("toggle"), icon, i18n.T(c, "sidebar_toggle"), a.hint("sidebar.toggle"), HeaderButton, false).
					OnClick(a.toggleNav)
			}
			a.trail(c)
			ui.Spacer(c)
			if a.Page == ConversationPage {
				a.pageActions(c)
			}
			if a.Hooks.PluginHeader != nil {
				a.Hooks.PluginHeader(c)
			}
			// Preview data is identified wherever it shows.
			// An opaque fill keeps it legible over the translucent band.
			if a.Preview {
				preview := ui.Row(c.Key("preview")).Radius(999).Shrink(0).Background(p.Background)
				kit.Tooltip(c, preview, i18n.T(c, "preview_notice"))
				preview.Children(func() {
					kit.Tag(c, i18n.T(c, "preview"))
				})
			}
		})
		a.headerSide(c, f, HeaderInsets(bar))
	})
}

// pageActions are the conversation page's own header actions, at the end of
// the main region: Find in the conversation, then the session overview.
func (a *App) pageActions(c *ui.Context) {
	cv := a.Data.Find(a.Sidebar.Selected)
	if cv == nil {
		return
	}
	kit.IconButtonHint(c.Key("find"), icons.Search, i18n.T(c, "chat_search"), a.hint("conversation.find"), HeaderButton, a.Messages.SearchOpen()).
		OnClick(func() { a.Messages.OpenSearch(cv.ID) })
	if a.assetsOf != cv.ID {
		// The popover is one conversation's: another starts closed.
		a.assets, a.assetsOf = composer.OverviewState{}, cv.ID
	}
	composer.OverviewButton(c, &a.assets, a.sessionOverview(c, cv), HeaderButton)
}

// sessionOverview is cv's overview: the live controller's, or the preview's
// made from its transcript, whose rows only say they are a preview.
func (a *App) sessionOverview(c *ui.Context, cv *preview.Conversation) *composer.Overview {
	if !a.Preview {
		if a.Hooks.Overview != nil {
			return a.Hooks.Overview(c, cv)
		}
		return &composer.Overview{}
	}
	o := preview.Overview(cv)
	act := &actions{a: a, c: c, cv: cv, comp: a.Composer(cv.ID)}
	o.OpenChild = act.OpenChild
	o.OpenAsset = func(string) { act.notice() }
	o.OpenSource = func(string) { act.notice() }
	return o
}

// HeaderInsets is the room the header keeps clear at the window's right
// edge: 12, past the window buttons MyGo reports there (Windows and Linux;
// none on macOS, whose lights are on the left).
func HeaderInsets(bar ui.TitleBar) float32 { return 12 + bar.Right }

// headerSide is the header from the split to the window's right edge. It is
// as wide as the panel shows, so its left edge is the panel's, and holds the
// panel's own header content, then the fixed cluster: the session status and
// the fold toggle, end clear of the window's right edge. With the panel
// closed only the cluster is left, at the end of the main region.
func (a *App) headerSide(c *ui.Context, f frame, end float32) {
	p := theme.Of(c)
	shown := f.panelW * f.details
	ui.Row(c.Key("side")).Height(HeaderHeight).Shrink(0).MinWidth(shown).PaddingEnd(end).Gap(ButtonGap).Children(func() {
		if f.details > 0.001 {
			// A short divider over the panel column's left border, as the
			// one over the sidebar's edge: same height and colour.
			ui.Box(c.Key("split")).Absolute().Right(shown-1).Top((HeaderHeight-dividerHeight)/2).Size(1, dividerHeight).
				Background(p.TitleBarBorder)
			// The segment asks for no width of its own and takes what the
			// column's width leaves beside the cluster, so the side is never
			// wider than the column for its tabs.
			seg := ui.Row(c.Key("segment")).Width(0).Grow(1).MinWidth(0).Height(HeaderHeight).ClipX().Gap(4)
			seg.Children(func() { a.panelHeader(c, seg) })
		}
		if a.Page == ConversationPage {
			a.strip(c)
		}
		// The fold toggle is the right column's only close control. It cannot
		// act, and is disabled, while the page has nothing to show there or
		// the window has no room for the column: the choice stays and shows
		// when there is room.
		kit.IconButtonHint(c.Key("details"), icons.PanelRight, i18n.T(c, "details"), a.hint("panel.toggle"), HeaderButton, f.showsPanel).
			Disabled(!f.fitted || !a.hasDetails()).OnClick(a.togglePanel)
	})
}

// hasDetails reports whether the page has something for the right column:
// the conversation's panel, the host's metrics, or a selected extension's
// details over Settings and the Market.
func (a *App) hasDetails() bool {
	return a.Page == ConversationPage || a.Page == HostPage || a.extensionDetails()
}

// panelHeader is the header segment over the right column, owned by what the
// column shows: the details' title, or the panel's tabs and its "+".
func (a *App) panelHeader(c *ui.Context, seg ui.Element) {
	p := theme.Of(c)
	title := func(text string) {
		seg.PaddingStart(12)
		ui.Text(c, text).FontSize(14).SingleLine().Shrink(1).MinWidth(0).TextColor(p.Foreground)
	}
	if a.extensionDetails() {
		title(settings.DetailsTitle(c, a.Settings))
		return
	}
	if a.Page == HostPage {
		title(i18n.T(c, "metrics_title"))
		return
	}
	seg.PaddingStart(8)
	a.syncPanel()
	panel.Strip(c, a.Panel, a.project(), kit.KeyText)
	// The tabs are the panel's: ⌘W with the focus on one closes the active
	// tab, as it does inside the column. The segment itself takes no focus,
	// so its gaps still drag the window.
	if seg.Shortcut(CloseFocused.Mods, CloseFocused.Key) && !c.ShortcutRepeated() {
		a.closeTab(c)
	}
}

// trail is the page's title: on a conversation in a project, the project
// and the conversation as breadcrumbs, the project revealing its rows in
// the sidebar.
func (a *App) trail(c *ui.Context) {
	p := theme.Of(c)
	if a.Hooks.PluginTitle != nil && a.Hooks.PluginTitle(c) {
		return
	}
	if a.Page == NotificationsPage {
		ui.Text(c, i18n.T(c, "notification_center")).FontSize(14).SingleLine().TextColor(p.Foreground)
		return
	}
	if a.Page == SettingsPage {
		ui.Text(c, settings.Title(c, a.Settings)).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
		return
	}
	if a.Page == HostPage {
		h, _ := a.hostPage()
		ui.Text(c, a.hostName(c, h.Name)).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
		return
	}
	if a.Page == ProjectPage {
		label := i18n.T(c, "workspace_project")
		if project := a.overviewProject(); project != nil {
			label = project.Name
		}
		ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
		return
	}
	if id := a.feature(); a.Page != ConversationPage && id != "" {
		f, _ := a.lookupFeature(id)
		label := i18n.T(c, f.Label)
		pr := a.Data.ProjectOf(a.Sidebar.Selected)
		if pr == nil || a.Page != FilesPage && a.Page != GitPage {
			ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
			return
		}
		// A project page: the project and the page. The page has no side
		// column, so the project goes back to the conversation with its rows
		// revealed there.
		chosen := -1
		if ui.Breadcrumbs(c.Key("trail"), []string{pr.Name, label}, &chosen).Label(i18n.T(c, "projects")).Shrink(1).MinWidth(0).FontSize(14).Changed() && chosen == 0 {
			a.Page, a.NavOpen = ConversationPage, true
			a.Sidebar.Reveal(pr.ID)
		}
		return
	}
	if a.panes.Displayed() != nil {
		// A split's panes have their own titles: the band names the page.
		ui.Text(c, i18n.T(c, "shell_sessions")).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
		return
	}
	cv := a.Data.Find(a.Sidebar.Selected)
	if cv == nil {
		ui.Text(c, i18n.T(c, "app")).FontSize(14).SingleLine().TextColor(p.Foreground)
		return
	}
	pr := a.Data.ProjectOf(cv.ID)
	if pr == nil {
		ui.Text(c, sidebar.Title(c, cv)).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
		return
	}
	chosen := -1
	items := []string{pr.Name, sidebar.Title(c, cv)}
	if ui.Breadcrumbs(c.Key("trail"), items, &chosen).Label(i18n.T(c, "projects")).Shrink(1).MinWidth(0).FontSize(14).Changed() && chosen == 0 {
		a.NavOpen = true
		a.Sidebar.Reveal(pr.ID)
	}
}

// dividerHeight is the height of the band's two short dividers, over the
// sidebar's edge and over the panel's: both in the title bar's border colour.
const dividerHeight = 16
