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
	CLI            func(*viewmodel.Project)
	// NewTerminal starts a terminal in the project's main worktree and shows
	// its tab. Terminals lists the project's terminals the Node has not
	// closed, and OpenTerminal shows one's tab.
	NewTerminal  func(*viewmodel.Project)
	Terminals    func() []ProjectTerminal
	OpenTerminal func(id string)
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
				for _, key := range []string{"workspace_new_terminal", "cli_launch"} {
					label := i18n.T(c, key)
					available := key == "cli_launch" && state.actions.CLI != nil || key == "workspace_new_terminal" && state.actions.NewTerminal != nil
					button := kit.SecondaryLook(c, ui.ButtonBase(c.Key(key)).Label(label)).Disabled(!available).Children(func() {
						ui.Icon(c, icons.Terminal).FontSize(16)
						ui.Text(c, label).FontSize(14)
					})
					if available && key == "workspace_new_terminal" {
						button.OnClick(func() { state.actions.NewTerminal(project) })
					} else if available {
						button.OnClick(func() { state.actions.CLI(project) })
					} else {
						kit.Tooltip(c, button, i18n.T(c, "unavailable"))
					}
				}
			})
			a.projectRecordsView(c, project, state)
		})
	})
	state.initializeDialog(c)
}

func (a *App) projectRecordsView(c *ui.Context, project *viewmodel.Project, state *projectOverview) {
	palette := theme.Of(c)
	ui.Column(c.Key("project-records")).FillWidth().Gap(12).Padding(16, 0, 0, 0).Children(func() {
		ui.Row(c.Key("toolbar")).Gap(16).Wrap().Children(func() {
			kit.Segmented(c, "tabs", &state.tab, []string{i18n.T(c, "workspace_sessions"), i18n.T(c, "terminal")}, kit.SegmentedProps{})
			ui.Spacer(c)
			label := i18n.T(c, "search")
			ui.SearchField(c.Key("search"), &state.search).Placeholder(label).Label(label).Width(240).MinWidth(0)
		})
		ui.Column(c.Key("records")).FillWidth().Border(1, palette.Border).Radius(palette.RadiusLg).Padding(8).Children(func() {
			query := strings.ToLower(strings.TrimSpace(state.search))
			shown := 0
			if state.tab == 1 {
				if state.actions.Terminals == nil {
					kit.Result(c, kit.ResultProps{Icon: icons.Terminal, Title: i18n.T(c, "unavailable")}).FillWidth()
					return
				}
				for _, terminal := range state.actions.Terminals() {
					title := terminal.Title
					if title == "" {
						title = i18n.T(c, "terminal")
					}
					if query != "" && !strings.Contains(strings.ToLower(title), query) && !strings.Contains(strings.ToLower(terminal.Path), query) {
						continue
					}
					shown++
					kit.Ghost(c.Key("terminal/"+terminal.ID), title, false).FillWidth().Padding(12).Gap(12).Justify(ui.Start).
						Disabled(state.actions.OpenTerminal == nil).Children(func() {
						ui.Icon(c, icons.Terminal).FontSize(16).Shrink(0).TextColor(palette.MutedForeground)
						ui.Column(c.Key("details")).Grow(1).MinWidth(0).Gap(4).Children(func() {
							ui.Text(c, title).FontSize(14).SingleLine()
							ui.Text(c, terminal.Path).FontSize(12).SingleLine().TextColor(palette.MutedForeground)
						})
					}).OnClick(func() { state.actions.OpenTerminal(terminal.ID) })
				}
				if shown == 0 {
					key := "workspace_no_terminal"
					if query != "" {
						key = "connection_search_empty"
					}
					kit.Result(c, kit.ResultProps{Icon: icons.Terminal, Title: i18n.T(c, key)}).FillWidth()
				}
				return
			}
			for _, session := range project.Conversations {
				if (!a.Preview && session.Node != state.node) || (state.actions.VisibleSession != nil && !state.actions.VisibleSession(session)) {
					continue
				}
				title := sidebar.Title(c, session)
				path := session.WorktreePath
				if path == "" {
					path = project.Path
				}
				if query != "" && !strings.Contains(strings.ToLower(title), query) && !strings.Contains(strings.ToLower(path), query) {
					continue
				}
				shown++
				kit.Ghost(c.Key(session.Node+"/"+session.ID), title, false).FillWidth().Padding(12).Gap(12).Justify(ui.Start).
					Disabled(state.pending || state.actions.OpenSession == nil).Children(func() {
					if state.pending && state.opening == session.Node+"/"+session.ID {
						kit.Busy(c)
					} else {
						ui.Icon(c, icons.Bot).FontSize(16).Shrink(0).TextColor(palette.MutedForeground)
					}
					ui.Column(c.Key("details")).Grow(1).MinWidth(0).Gap(4).Children(func() {
						ui.Text(c, title).FontSize(14).SingleLine()
						ui.Text(c, path).FontSize(12).SingleLine().TextColor(palette.MutedForeground)
					})
				}).OnClick(func() {
					state.pending, state.opening = true, session.Node+"/"+session.ID
					state.actions.OpenSession(session, func(bool) { state.pending, state.opening = false, "" })
				})
			}
			if shown == 0 {
				key := "project_sessions_empty"
				if query != "" {
					key = "connection_search_empty"
				}
				kit.Result(c, kit.ResultProps{Icon: icons.Bot, Title: i18n.T(c, key)}).FillWidth()
			}
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
	if !state.initialize {
		return
	}
	palette := theme.Of(c)
	label := i18n.T(c, "git_initialize")
	kit.Dialog(c.Key("initialize-project"), &state.initialize, func() {
		ui.Text(c, label).FontSize(16).FontWeight(600).TextColor(palette.PopoverForeground)
		ui.Text(c, i18n.T(c, "git_initialize_nested_confirm")).FontSize(14).TextColor(palette.MutedForeground)
		ui.Text(c, strings.Join(state.repositories, "\n")).FontSize(14).Selectable()
		ui.Row(c.Key("actions")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(state.pending).OnClick(func() { state.initialize = false })
			kit.PrimaryBusy(c.Key("initialize"), label, false, state.pending).OnClick(state.initializeGit)
		})
	}).Width(560).Label(label)
}
