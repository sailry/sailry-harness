package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// header is the band above the rail and the columns: the app's name over the
// sidebar, clear of the traffic lights, then the page's header: the sidebar
// toggle, the breadcrumbs, the activity strip, the preview tag and the
// details toggle. It drags the window.
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
				ui.Box(c.Key("divider")).Absolute().Right(0).Top((HeaderHeight-16)/2).Size(1, 16).
					Background(p.Border).Opacity(f.nav)
			}
		})
		pad := max(lights-left, 0) + 12
		ui.Row(c.Key("module")).Grow(1).MinWidth(0).Height(HeaderHeight).PaddingStart(pad).PaddingEnd(12 + bar.Right).Gap(8).Children(func() {
			icon := icons.PanelLeftClose
			if !a.NavOpen {
				icon = icons.PanelLeftOpen
			}
			kit.IconButtonHint(c.Key("toggle"), icon, i18n.T(c, "sidebar_toggle"), a.hint("sidebar.toggle"), 28, false).
				OnClick(func() { a.NavOpen = !a.NavOpen })
			a.trail(c)
			ui.Spacer(c)
			if a.Page == ConversationPage {
				a.strip(c)
			}
			// Preview data is identified wherever it shows.
			// An opaque fill keeps it legible over the translucent band.
			if a.Preview {
				preview := ui.Row(c.Key("preview")).Radius(999).Background(p.Background)
				kit.Tooltip(c, preview, i18n.T(c, "preview_notice"))
				preview.Children(func() {
					kit.Tag(c, i18n.T(c, "preview"))
				})
			}
			if a.Page == ConversationPage || a.Page == HostPage {
				label := i18n.T(c, "details")
				b := kit.IconButtonHint(c.Key("details"), icons.PanelRight, label, a.hint("panel.toggle"), 28, f.showsPanel).OnClick(a.togglePanel)
				if !f.fitted {
					// Too narrow: the panel stays chosen and shows when there is room.
					b.Opacity(0.5)
				}
			}
		})
	})
}

// trail is the page's title: on a conversation in a project, the project
// and the conversation as breadcrumbs, the project revealing its rows in
// the sidebar.
func (a *App) trail(c *ui.Context) {
	p := theme.Of(c)
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
		ui.Text(c, i18n.T(c, h.Name)).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
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
		f, _ := featureOf(id)
		label := i18n.T(c, f.Label)
		pr := a.Data.ProjectOf(a.Sidebar.Selected)
		if pr == nil || a.Page != FilesPage && a.Page != GitPage {
			ui.Text(c, label).FontSize(14).SingleLine().Shrink(1).TextColor(p.Foreground)
			return
		}
		// A project page: the project, revealing its rows, and the page.
		chosen := -1
		if ui.Breadcrumbs(c.Key("trail"), []string{pr.Name, label}, &chosen).Label(i18n.T(c, "projects")).Shrink(1).MinWidth(0).FontSize(14).Changed() && chosen == 0 {
			a.NavOpen = true
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
