package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
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
	left := f.rail + f.navWidth*f.nav
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
		// Without the lights (Windows, Linux) nothing else names the window,
		// so the name stays on every page: the column is at least as wide as
		// it, whether the sidebar shows or not. With them it fades with the
		// sidebar, the lights taking its place.
		name, shown, brand := i18n.T(c, "app"), f.nav, left
		if lights == 0 {
			w, _ := c.MeasureText(0, ui.Span{Text: name, Size: 14})
			shown, brand = 1, max(left, w+24)
		}
		lead := min(max(lights, 12), max(brand-12, 0))
		ui.Row(c.Key("brand")).Width(brand).Height(HeaderHeight).PaddingStart(lead).PaddingEnd(12).ClipX().Children(func() {
			if shown > 0.001 {
				ui.Text(c, name).FontSize(14).SingleLine().TextColor(p.SidebarForeground).Opacity(shown)
				// A short divider over the sidebar's edge, apart from the
				// edge's own line below the band.
				ui.Box(c.Key("divider")).Absolute().Right(0).Top((HeaderHeight-dividerHeight)/2).Size(1, dividerHeight).
					Background(p.TitleBarBorder).Opacity(shown)
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
				if a.Page == HostPage {
					kit.MenuButton(c.Key("host-more"), "host-more-"+a.Host, func(b ui.Element) {
						kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "more"), "", HeaderButton, false)
					}, func(m *kit.Menu) { a.hostMenu(c, m) }, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
				}
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
				if a.panes.Displayed() == nil && a.panes.Active.Kind != panes.Terminal {
					cv = a.Data.Find(a.Sidebar.Selected)
				}
				if hasActions(cv) {
					known += 2 * (HeaderButton + ButtonGap)
				}
				// The title and the session status share what the other
				// controls leave: the title at its start, the status at its
				// end, directly before the page's actions.
				lead := ui.Row(c.Key("lead")).Grow(1).MinWidth(0).Height(HeaderHeight).Gap(ButtonGap)
				room := a.band.room(c, lead, width-brand-sideWidth(f, end)-pad-padEnd, known)
				// The tabs take the title's place and give way as it
				// does: they shorten first, then the strip's groups fold.
				plan := a.planStrip(c, room)
				free := room - ButtonGap
				if len(plan.lanes) > 0 {
					free -= plan.width + ButtonGap
				}
				groups := a.workspaceGroupWidth(c, free)
				lead.Children(func() {
					if groups > 0 {
						a.workspaceGroups(c, groups)
					} else {
						// The title is cut at its own edge: breadcrumbs
						// keep a least width per item, which must not show
						// over the status.
						ui.Row(c.Key("title")).Shrink(1).MinWidth(0).Height(HeaderHeight).ClipX().Children(func() { a.trail(c) })
					}
					ui.Spacer(c)
					a.strip(c, plan)
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
	entry := a.overviewEntry(cv.ID)
	overview := a.conversationOverview(c, cv, entry.act)
	entry.state.Docked = a.overviewDocked(overview)
	composer.OverviewButton(c, &entry.state, overview, size, &a.OverviewHidden)
}

// overviewPopover is a conversation's session overview popover, kept while
// its button shows.
type overviewPopover struct {
	state composer.OverviewState
	// act is the conversation's actions as its view built them last,
	// which review a changed file from the header's popover.
	act ConversationActions
	// shown is whether this frame built its button.
	shown bool
	// opened is the owner callback that admitted the visible inventory read.
	opened func(bool)
}

func (o *overviewPopover) close() {
	opened := o.opened
	o.opened = nil
	if opened != nil {
		opened(false)
	}
}

func (o *overviewPopover) observe(value *composer.Overview) *composer.Overview {
	copy := *value
	opened := value.Opened
	copy.Opened = func(open bool) {
		if !open {
			o.close()
			return
		}
		if o.opened == nil && opened != nil {
			o.opened = opened
			opened(true)
		}
	}
	return &copy
}

// overviewEntry is conversation id's overview popover: each conversation has
// its own, so one opening in a pane leaves another pane's as it is.
func (a *App) overviewEntry(id string) *overviewPopover {
	o := a.overviews[id]
	if o == nil {
		if a.overviews == nil {
			a.overviews = map[string]*overviewPopover{}
		}
		o = &overviewPopover{}
		a.overviews[id] = o
	}
	o.shown = true
	return o
}

// sweepOverviews forgets, at the start of a frame, the popovers whose
// button the last frame did not build: a conversation that comes back
// starts closed.
func (a *App) sweepOverviews() {
	for id, o := range a.overviews {
		if !o.shown {
			delete(a.overviews, id)
			o.close()
			continue
		}
		o.shown = false
	}
}

// sessionOverview is cv's overview: the live controller's, or the preview's
// made from its transcript, whose rows only say they are a preview (the
// demo's say nothing).
func (a *App) sessionOverview(c *ui.Context, cv *preview.Conversation) *composer.Overview {
	var o *composer.Overview
	if !a.sampled() {
		if a.Hooks.Overview != nil {
			o = a.Hooks.Overview(c, cv)
		} else {
			o = &composer.Overview{}
		}
		return a.overviewEntry(cv.ID).observe(o)
	}
	o = preview.Overview(cv)
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
	return a.overviewEntry(cv.ID).observe(o)
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
		label := i18n.T(c, "details")
		if a.pluginDetails() && a.PluginDetailsLabel != "" {
			label = a.PluginDetailsLabel
		}
		kit.IconButtonHint(c.Key("details"), icons.PanelRight, label, a.hint("panel.toggle"), HeaderButton, f.showsPanel).
			Disabled(!f.fitted || !a.hasDetails()).OnClick(a.togglePanel)
	})
}

// hasDetails reports whether the page has something for the right column:
// the conversation's panel, the host's metrics, a selected extension's
// details over Settings and the Market, or workspace resource details.
func (a *App) hasDetails() bool {
	return a.Page == ConversationPage || a.Page == HostPage || a.extensionDetails() || a.workspaceDetails()
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
	if a.pluginDetails() {
		if a.PluginDetailsHeader != nil {
			seg.PaddingStart(12)
			a.PluginDetailsHeader(c)
		}
		return
	}
	if a.Page == HostPage {
		title(i18n.T(c, "metrics_title"))
		return
	}
	if a.paged() != "" {
		// The resource toolbar names its tree or branch and keeps its
		// actions together; do not repeat that title in the window band.
		return
	}
	if a.Page != ConversationPage || !a.PanelOpen {
		return
	}
	seg.PaddingStart(8)
	panel.Strip(c, a.Panel, a.panels.project, kit.KeyText)
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
		if a.paged() == "" {
			ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
			return
		}
		ui.Row(c.Key("trail")).Gap(4).AlignItems(ui.Center).Shrink(1).MinWidth(0).Children(func() {
			a.projectSwitcher(c, a.ResourceProject())
			ui.Icon(c, icons.ChevronRight).FontSize(14).Shrink(0).TextColor(p.MutedForeground)
			ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).MinWidth(0).TextColor(p.Foreground)
		})
		return
	}
	tabs := a.headerTabs(c)
	if len(tabs) > 1 {
		// The band's tabs name what shows; a split's panes have their own titles.
		return
	}
	if len(tabs) == 1 && tabs[0].group != nil && tabs[0].shown {
		ui.Text(c, tabs[0].title).FontSize(14).SingleLine().Shrink(1).MinWidth(0).TextColor(p.Foreground)
		return
	}
	if a.panes.Active.Kind == panes.Terminal {
		ui.Text(c, a.paneTitle(c, a.panes.Active)).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
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

// projectSwitcher selects a Files and Git workspace without moving the chat.
func (a *App) projectSwitcher(c *ui.Context, current *preview.Project) {
	p := theme.Of(c)
	target := a.ResourceTarget()
	shown := i18n.T(c, "shell_sessions")
	if current != nil {
		shown = current.Name
	} else if target.Project != "" {
		shown = i18n.T(c, "unavailable")
	}
	if target.Project == "" && len(a.Data.Projects) == 0 {
		ui.Text(c, shown).FontSize(14).SingleLine().Shrink(1).TextColor(p.MutedForeground)
		return
	}
	kit.MenuButton(c.Key("project"), "project-switch", func(b ui.Element) {
		kit.GhostLook(c, b, false).Row().Height(28).Gap(4).PaddingX(8).Shrink(1).MinWidth(0).Label(i18n.T(c, "projects")).Children(func() {
			ui.Text(c, shown).FontSize(14).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
			ui.Icon(c, icons.ChevronDown).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
		})
	}, func(m *kit.Menu) {
		if m.Item(i18n.T(c, "resource_follow_conversation")).Checked(target.Project == "").Chosen() {
			a.SetResourceTarget(ResourceTarget{})
		}
		if len(a.Data.Projects) > 0 {
			m.Separator()
		}
		for _, project := range a.Data.Projects {
			active := target.Node == project.Host && target.Project == project.ID
			item := m.Item(project.Name).Checked(active)
			hostName := project.Host[:min(len(project.Host), 10)]
			for _, host := range a.Data.Hosts {
				if host.ID == project.Host && host.Name != "" {
					hostName = a.hostName(c, host.Name)
					break
				}
			}
			if project.Offline {
				if hostName != "" {
					hostName += " · "
				}
				hostName += i18n.T(c, "resource_offline")
			}
			if hostName != "" {
				item.Trailing(hostName)
			}
			if item.Chosen() && !active {
				a.SetResourceTarget(ResourceTarget{Node: project.Host, Project: project.ID})
			}
		}
	}, kit.MenuOptions{At: ui.AnchorBottomStart, Self: ui.AnchorTopStart, MaxHeight: 320})
}
