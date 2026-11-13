package shell

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/sidebar"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type ProjectRepository struct {
	Known, Directory bool
	Branch           string
	Repositories     []string
}

type ProjectActions struct {
	NewSession     func(*viewmodel.Project)
	VisibleSession func(*viewmodel.Conversation) bool
	OpenSession    func(*viewmodel.Conversation, func(bool))
	Repository     func() ProjectRepository
	Initialize     func(func(bool))
	// NewTerminal starts a terminal in the project's main worktree and shows
	// its tab. Terminals lists the project's terminals the Node has not
	// closed, and OpenTerminal shows one's tab.
	NewTerminal  func(*viewmodel.Project)
	Terminals    func() []ProjectTerminal
	OpenTerminal func(id string)
	// Controls draws the controls packages add to the page, after its own.
	Controls func(*ui.Context)
}

// ProjectTerminal is one row of a project's Terminal records.
type ProjectTerminal struct {
	ID, Title, Path string
}

type projectOverview struct {
	node, project string
	tab           int
	search        string
	pending       bool
	opening       string // the session row whose opening is pending
	initialize    bool
	repositories  []string
	actions       ProjectActions
}

func (a *App) OpenProject(project *viewmodel.Project, actions ProjectActions) {
	if project == nil {
		return
	}
	a.overview = &projectOverview{node: project.Host, project: project.ID, actions: actions}
	a.Host = project.Host
	a.Navigate(ProjectPage)
}

func (a *App) ProjectOwner() (node, project string, shown bool) {
	if a.Page != ProjectPage || a.overview == nil {
		return "", "", false
	}
	return a.overview.node, a.overview.project, true
}

func (a *App) overviewProject() *viewmodel.Project {
	if a.overview == nil {
		return nil
	}
	for _, project := range a.Data.Projects {
		if project.Host == a.overview.node && project.ID == a.overview.project {
			return project
		}
	}
	return nil
}

func (a *App) projectView(c *ui.Context) {
	project := a.overviewProject()
	if project == nil {
		a.unavailable(c)
		return
	}
	state := a.overview
	palette := theme.Of(c)
	ui.Scroll(c.Key("project-overview")).Grow(1).MinHeight(0).Padding(24).Children(func() {
		ui.Column(c.Key("content")).FillWidth().MaxWidth(theme.ContentWidth).Margin(16, ui.Auto).Gap(16).Children(func() {
			ui.Row(c.Key("heading")).Gap(12).Children(func() {
				ui.Text(c, project.Name).FontSize(20).FontWeight(600).SingleLine().Shrink(1).MinWidth(0)
				for _, host := range a.Data.Hosts {
					if host.ID == project.Host {
						ui.Text(c, a.hostName(c, host.Name)).FontSize(14).SingleLine().Shrink(1).TextColor(palette.MutedForeground)
						break
					}
				}
			})
			ui.Text(c, project.Path).FontSize(14).SingleLine().TextColor(palette.MutedForeground)
			repository := ProjectRepository{Known: project.Branch != "", Branch: project.Branch}
			if state.actions.Repository != nil {
				repository = state.actions.Repository()
			}
			ui.Row(c.Key("repository")).Gap(8).Children(func() {
				if repository.Known && repository.Directory {
					label := i18n.T(c, "git_initialize")
					kit.Ghost(c.Key("initialize"), label, false).Height(28).PaddingX(8).Gap(6).
						Disabled(state.pending || state.actions.Initialize == nil).Children(func() {
						if state.pending && state.opening == "" && !state.initialize {
							kit.Busy(c)
						} else {
							ui.Icon(c, icons.Network).FontSize(16)
						}
						ui.Text(c, label).FontSize(14)
					}).OnClick(func() {
						if len(repository.Repositories) > 0 {
							state.repositories = append([]string(nil), repository.Repositories...)
							state.initialize = true
						} else {
							state.initializeGit()
						}
					})
				} else if repository.Branch != "" {
					ui.Icon(c, icons.Network).FontSize(16).TextColor(palette.MutedForeground)
					ui.Text(c, repository.Branch).FontSize(14).TextColor(palette.MutedForeground)
				}
			})
			ui.Row(c.Key("actions")).Gap(8).Wrap().Children(func() {
				label := i18n.T(c, "workspace_new_session")
				kit.Primary(c.Key("new-session"), label, state.actions.NewSession == nil).Children(func() {
					ui.Icon(c, icons.Plus).FontSize(16)
					ui.Text(c, label).FontSize(14)
				}).OnClick(func() { state.actions.NewSession(project) })
				terminal := i18n.T(c, "workspace_new_terminal")
				button := kit.SecondaryLook(c, ui.ButtonBase(c.Key("workspace_new_terminal")).Label(terminal)).Disabled(state.actions.NewTerminal == nil).Children(func() {
					ui.Icon(c, icons.Terminal).FontSize(16)
					ui.Text(c, terminal).FontSize(14)
				})
				if state.actions.NewTerminal != nil {
					button.OnClick(func() { state.actions.NewTerminal(project) })
				} else {
					kit.Tooltip(c, button, i18n.T(c, "unavailable"))
				}
				if state.actions.Controls != nil {
					state.actions.Controls(c)
				}
			})
			a.projectRecordsView(c, project, state)
		})
	})
	state.initializeDialog(c)
}

// projectRecordsView is the project's records: its sessions or its
// terminals, searched, in the list Settings pages use (kit.List: the filled
// card, rows apart by hairlines, the empty state inside it).
func (a *App) projectRecordsView(c *ui.Context, project *viewmodel.Project, state *projectOverview) {
	ui.Column(c.Key("project-records")).FillWidth().Gap(12).Padding(16, 0, 0, 0).Children(func() {
		ui.Row(c.Key("toolbar")).Gap(16).Wrap().Children(func() {
			kit.Segmented(c, "tabs", &state.tab, []string{i18n.T(c, "workspace_sessions"), i18n.T(c, "terminal")}, kit.SegmentedProps{})
			ui.Spacer(c)
			label := i18n.T(c, "search")
			ui.SearchField(c.Key("search"), &state.search).Placeholder(label).Label(label).Width(240).MinWidth(0)
		})
		query := strings.ToLower(strings.TrimSpace(state.search))
		matches := func(title, path string) bool {
			return query == "" || strings.Contains(strings.ToLower(title), query) || strings.Contains(strings.ToLower(path), query)
		}
		var rows []func()
		icon, none := icons.Bot, "project_sessions_empty"
		switch {
		case state.tab == 1 && state.actions.Terminals == nil:
			icon, none, query = icons.Terminal, "unavailable", ""
		case state.tab == 1:
			icon, none = icons.Terminal, "workspace_no_terminal"
			for _, terminal := range state.actions.Terminals() {
				title := terminal.Title
				if title == "" {
					title = i18n.T(c, "terminal")
				}
				if !matches(title, terminal.Path) {
					continue
				}
				rows = append(rows, func() {
					projectRecord(c, "terminal/"+terminal.ID, title, terminal.Path, icons.Terminal, false, nil).
						Disabled(state.actions.OpenTerminal == nil).OnClick(func() { state.actions.OpenTerminal(terminal.ID) })
				})
			}
		default:
			for _, session := range project.Conversations {
				if (!a.sampled() && session.Node != state.node) || (state.actions.VisibleSession != nil && !state.actions.VisibleSession(session)) {
					continue
				}
				title := sidebar.Title(c, session)
				path := session.WorktreePath
				if path == "" {
					path = project.Path
				}
				if !matches(title, path) {
					continue
				}
				id := session.Node + "/" + session.ID
				rows = append(rows, func() {
					projectRecord(c, id, title, path, nil, state.pending && state.opening == id, project).
						Disabled(state.pending || state.actions.OpenSession == nil).
						ContextMenu(func(m *ui.Menu) { a.Sidebar.SessionMenu(c, m, session) }).OnClick(func() {
						state.pending, state.opening = true, id
						state.actions.OpenSession(session, func(bool) { state.pending, state.opening = false, "" })
					})
				})
			}
		}
		if query != "" {
			none = "connection_search_empty"
		}
		kit.List(c, len(rows), func(i int) { rows[i]() }, func() {
			kit.Result(c, kit.ResultProps{Icon: icon, Title: i18n.T(c, none)}).FillWidth()
		})
	})
}

// projectRecord is a row of the records' card that opens something, at the
// Settings row metrics: the icon in its slot (a spinner while busy), the
// title over the path, and the neutral hover fill across the card. A
// session's row leads with its project's glyph (user decision 2026-11-12)
// in place of icon.
func projectRecord(c *ui.Context, key, title, path string, icon *ui.SVG, busy bool, project *viewmodel.Project) ui.Element {
	palette := theme.Of(c)
	row := ui.ButtonBase(c.Key(key)).Label(title).Padding(kit.RowPad, 16).Margin(0, -16).Gap(kit.RowIconGap).MinHeight(kit.RowMinHeight).
		Justify(ui.Start).Cursor(ui.CursorPointer).TextColor(palette.SurfaceForeground)
	if row.Hovered() {
		row.Background(palette.Hover)
	}
	return row.Children(func() {
		ui.Row(c.Key("lead")).Size(kit.RowIcon, kit.RowIcon).Shrink(0).Center().Children(func() {
			if busy {
				kit.Busy(c)
			} else if project != nil {
				kit.ProjectGlyph(c, project.Icon, project.Color, 18)
			} else {
				ui.Icon(c, icon).FontSize(18).TextColor(palette.MutedForeground)
			}
		})
		ui.Column(c.Key("details")).Grow(1).Shrink(1).MinWidth(0).Gap(kit.TitleGap).Children(func() {
			ui.Text(c, title).FontSize(kit.RowFont).SingleLine()
			ui.Text(c, path).FontSize(kit.RowFont).SingleLine().TextColor(palette.Description())
		})
	})
}

func (state *projectOverview) initializeGit() {
	if state.actions.Initialize == nil || state.pending {
		return
	}
	state.pending = true
	state.actions.Initialize(func(ok bool) {
		state.pending = false
		if ok {
			state.initialize = false
		}
	})
}

func (state *projectOverview) initializeDialog(c *ui.Context) {
	if !state.initialize || state.pending {
		return
	}
	label := i18n.T(c, "git_initialize")
	open := state.initialize
	switch kit.Confirm(c.Key("initialize-project").Key(state), &open, kit.Confirmation{
		Title: label, Message: i18n.T(c, "git_initialize_nested_confirm") + "\n\n" + strings.Join(state.repositories, "\n"),
		Buttons: []string{i18n.T(c, "settings_cancel"), label}, Default: 1,
	}) {
	case 0:
		state.initialize = false
	case 1:
		state.initializeGit()
	}
}
