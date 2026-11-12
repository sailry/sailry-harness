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
// the title and, at its end, the session status and the page's own actions.
// The region over the panel is the panel's: its tabs or its title. The fold
// toggle alone is fixed at the window's far right, clear of the window
// buttons, in both states. Its empty areas drag the window.
func (a *App) header(c *ui.Context, f frame) {
	p := theme.Of(c)
	bar := c.TitleBar()
	width, _ := c.Size()
	end := HeaderInsets(bar)
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
		// Its padding stays inside that width: padding wider than the column
		// would widen it, and the lights' room would be counted again in the
		// page's header.
		lead := min(max(lights, 12), max(left-12, 0))
		ui.Row(c.Key("brand")).Width(left).Height(HeaderHeight).PaddingStart(lead).PaddingEnd(12).ClipX().Children(func() {
			if f.nav > 0.001 {
				ui.Text(c, i18n.T(c, "app")).FontSize(14).SingleLine().TextColor(p.SidebarForeground).Opacity(f.nav)
				// A short divider over the sidebar's edge, apart from the
				// edge's own line below the band.
				ui.Box(c.Key("divider")).Absolute().Right(0).Top((HeaderHeight-dividerHeight)/2).Size(1, dividerHeight).
					Background(p.TitleBarBorder).Opacity(f.nav)
			}
		})
		pad := max(lights-left, 0) + 12
		// The main region ends one gap before the fold toggle, and 12 before
		// the split while the panel shows.
		padEnd := ButtonGap + 4*f.details
		ui.Row(c.Key("module")).Grow(1).MinWidth(0).Height(HeaderHeight).PaddingStart(pad).PaddingEnd(padEnd).Gap(ButtonGap).Children(func() {
			// What the region's controls of a known width take, each with
			// the gap beside it.
			known := float32(0)
			// The toggle belongs to the side column: a page without one shows
			// none, as the reference's module header.
			if a.HasNavigation() {
				icon := icons.PanelLeftClose
				if !a.NavOpen {
					icon = icons.PanelLeftOpen
				}
				kit.IconButtonHint(c.Key("toggle"), icon, i18n.T(c, "sidebar_toggle"), a.hint("sidebar.toggle"), HeaderButton, false).
					OnClick(a.toggleNav)
				known += HeaderButton + ButtonGap
			}
			if a.Page != ConversationPage {
				a.band = band{}
				a.trail(c)
				ui.Spacer(c)
				if project := a.overviewProject(); a.Page == ProjectPage && project != nil {
					kit.MenuButton(c.Key("more"), "project-more", func(b ui.Element) {
						kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "more"), "", HeaderButton, false)
					}, func(m *kit.Menu) { a.Sidebar.ProjectActions(c, m, project) }, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
				}
			} else {
				// Find and the overview are one conversation's: the band
				// has them while the conversation shows alone, and each
				// pane of a split has its own (paneHeader).
				var cv *preview.Conversation
				if a.panes.Displayed() == nil {
					cv = a.Data.Find(a.Sidebar.Selected)
				}
				if hasActions(cv) {
					known += 2 * (HeaderButton + ButtonGap)
				}
				// The title and the session status share what the other
				// controls leave: the title at its start, the status at its
				// end, directly before the page's actions.
				lead := ui.Row(c.Key("lead")).Grow(1).MinWidth(0).Height(HeaderHeight).Gap(ButtonGap)
				room := a.band.room(c, lead, width-left-sideWidth(f, end)-pad-padEnd, known)
				lead.Children(func() {
					// The title gives way first and is cut at its own
					// edge: breadcrumbs keep a least width per item, which
					// must not show over the status.
					ui.Row(c.Key("title")).Shrink(1).MinWidth(0).Height(HeaderHeight).ClipX().Children(func() { a.trail(c) })
					ui.Spacer(c)
					a.strip(c, room)
				})
				a.conversationActions(c, cv, HeaderButton)
			}
			// Over a demo conversation the controller shows the statistics
			// package's context panel alone, read from the conversation's
			// own usage (live renderPluginHeader).
			if a.Hooks.PluginHeader != nil {
				a.Hooks.PluginHeader(c)
			}
			// Preview data is identified wherever it shows.
			// An opaque fill keeps it legible over the translucent band.
			if a.Preview && !a.Demo() {
				preview := ui.Row(c.Key("preview")).Radius(999).Shrink(0).Background(p.Background)
				kit.Tooltip(c, preview, i18n.T(c, "preview_notice"))
				preview.Children(func() {
					kit.Tag(c, i18n.T(c, "preview"))
				})
			}
		})
		a.headerSide(c, f, end)
	})
}

// titleRoom is the width the title keeps before the session status folds:
// the title shortens with an ellipsis first.
const titleRoom = 96

// band is the header's main region on the conversation page, as the
// session status fits into it. The region's width is known as the frame
// builds, from the columns; what its controls of no known width take (the
// plugins' contributions, the preview tag) is read from the last frame's
// layout.
type band struct {
	// built is whether the last frame laid the region out; inner and known
	// are its content width and its known controls' then, and extras what
	// the others took before that.
	built                bool
	inner, known, extras float32
}

// room is the width the title and the session status share in this frame:
// lead's, which takes what the region's other controls leave of inner. A
// frame built on a measure that has since changed asks for another, so the
// status settles in the layout it is shown in; a region at rest asks for
// none.
func (b *band) room(c *ui.Context, lead ui.Element, inner, known float32) float32 {
	extras := float32(0)
	if b.built {
		extras = max(b.inner-b.known-lead.LayoutBounds().W, 0)
	}
	if d := extras - b.extras; !b.built || d > 0.5 || d < -0.5 {
		c.AnimationFrame()
	}
	*b = band{built: true, inner: inner, known: known, extras: extras}
	return inner - known - extras
}

// sideWidth is the width of the header's side (headerSide): the column's
// while it shows, and at least the fold toggle before the window's edge.
func sideWidth(f frame, end float32) float32 {
	least := HeaderButton + end
	if f.details > 0.001 {
		// The segment over the column, one gap before the toggle.
		least += ButtonGap
	}
	return max(f.panelW*f.details, least)
}

// hasActions reports whether cv has its own actions: a conversation without
// a turn has nothing to find or to list.
func hasActions(cv *preview.Conversation) bool { return cv != nil && len(cv.Turns) > 0 }

// conversationActions are a conversation's own actions, size square: Find
// in it, then its session overview. They end the band's main region while
// the conversation shows alone, and are in its pane's header in a split,
// each pane's acting on its own conversation.
func (a *App) conversationActions(c *ui.Context, cv *preview.Conversation, size float32) {
	if !hasActions(cv) {
		return
	}
	kit.IconButtonHint(c.Key("find"), icons.Search, i18n.T(c, "chat_search"), a.hint("conversation.find"), size, a.Messages.SearchOpenIn(cv.ID)).
		OnClick(func() { a.Messages.OpenSearch(cv.ID) })
	composer.OverviewButton(c, a.overviewOf(cv.ID), a.sessionOverview(c, cv), size)
}

// overviewPopover is a conversation's session overview popover, kept while
// its button shows.
type overviewPopover struct {
	state composer.OverviewState
	// shown is whether this frame built its button.
	shown bool
}

// overviewOf is conversation id's overview popover: each conversation has
// its own, so one opening in a pane leaves another pane's as it is.
func (a *App) overviewOf(id string) *composer.OverviewState {
	o := a.overviews[id]
	if o == nil {
		if a.overviews == nil {
			a.overviews = map[string]*overviewPopover{}
		}
		o = &overviewPopover{}
		a.overviews[id] = o
	}
	o.shown = true
	return &o.state
}

// sweepOverviews forgets, at the start of a frame, the popovers whose
// button the last frame did not build: a conversation that comes back
// starts closed.
func (a *App) sweepOverviews() {
	for id, o := range a.overviews {
		if !o.shown {
			delete(a.overviews, id)
			continue
		}
		o.shown = false
	}
}

// sessionOverview is cv's overview: the live controller's, or the preview's
// made from its transcript, whose rows only say they are a preview (the
// demo's say nothing).
func (a *App) sessionOverview(c *ui.Context, cv *preview.Conversation) *composer.Overview {
	if !a.sampled() {
		if a.Hooks.Overview != nil {
			return a.Hooks.Overview(c, cv)
		}
		return &composer.Overview{}
	}
	o := preview.Overview(cv)
	act := &actions{a: a, c: c, cv: cv, comp: a.Composer(cv.ID)}
	o.OpenChild = act.OpenChild
	o.OpenAsset = func(id string) {
		// The demo's files open in the panel, as their cards do.
		if f, ok := preview.Asset(cv, id); ok && a.Demo() {
			act.OpenFile(f)
			return
		}
		act.notice()
	}
	o.OpenSource = func(uri string) {
		// The demo's cited page opens in the panel's browser, as a link of
		// the reply does.
		if a.Demo() {
			a.sampleLink(c, cv.ID, uri)
			return
		}
		act.notice()
	}
	return o
}

// HeaderInsets is the room the header keeps clear at the window's right
// edge: 12, past the window buttons MyGo reports there (Windows and Linux;
// none on macOS, whose lights are on the left).
func HeaderInsets(bar ui.TitleBar) float32 { return 12 + bar.Right }

// headerSide is the header from the split to the window's right edge. It is
// as wide as the panel shows (sideWidth), so its left edge is the panel's,
// and holds the panel's own header content, then the fold toggle alone, end
// clear of the window's right edge. With the panel closed only the toggle is
// left, at the end of the main region. The session status is the main
// region's (strip).
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
			// column's width leaves beside the toggle, so the side is never
			// wider than the column for its tabs.
			seg := ui.Row(c.Key("segment")).Width(0).Grow(1).MinWidth(0).Height(HeaderHeight).ClipX().Gap(4)
			seg.Children(func() { a.panelHeader(c, seg) })
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
