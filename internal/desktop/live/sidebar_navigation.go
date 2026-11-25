package live

import (
	"os"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) initSidebarNavigation() {
	r.App.Sidebar.Actions.CanArchive = r.canArchiveConversation
	r.App.Sidebar.Actions.Overview = r.openProjectOverview
	r.App.Sidebar.Actions.ProjectMenu = r.projectPluginMenu
	r.App.Sidebar.Actions.OpenWorkspace = func(project *viewmodel.Project, session *viewmodel.Conversation) {
		owner := session.Node
		if owner == "" {
			owner = project.Host
		}
		if owner != r.controller.Target().String() {
			r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
			return
		}
		path := session.WorktreePath
		if path == "" {
			path = project.Path
		}
		if _, err := os.Stat(path); err != nil {
			r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "files_open_failed")) })
			return
		}
		if r.attachments.native.OpenLocal != nil {
			r.attachments.native.OpenLocal(path)
		}
	}
}

func (r *Runtime) projectOverviewRuntime(project *viewmodel.Project) *Runtime {
	if project == nil {
		return nil
	}
	root := r.localRuntime()
	owner := root
	if project.Host != root.controller.Target().String() {
		owner = root.bindings[project.Host]
	}
	if owner == nil {
		return nil
	}
	for _, current := range owner.snapshot().Projects {
		if current.ID.String() == project.ID {
			return owner
		}
	}
	return nil
}

func (r *Runtime) openProjectOverview(project *viewmodel.Project) {
	r.openProjectOverviewAt(project, "")
}

func (r *Runtime) openProjectOverviewAt(project *viewmodel.Project, selectedPath string) {
	if project == nil {
		return
	}
	root := r.localRuntime()
	actions := shell.ProjectActions{
		NewSession: func(current *viewmodel.Project) {
			if current.Offline {
				root.newDraft(current)
				if selectedPath != "" {
					root.App.Composer(root.App.Sidebar.Selected).Worktree = selectedPath
				}
				return
			}
			owner := root.projectOverviewRuntime(current)
			if owner == nil {
				root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
				return
			}
			if _, ok := owner.projectWorktree(current, selectedPath); !ok {
				root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
				return
			}
			root.newDraft(current)
			if selectedPath != "" {
				root.App.Composer(root.App.Sidebar.Selected).Worktree = selectedPath
			}
		},
		VisibleSession: func(cv *viewmodel.Conversation) bool {
			owner := root.projectOverviewRuntime(project)
			if owner == nil {
				return false
			}
			session, ok := owner.session(cv.ID)
			return ok && session.Project != nil && session.Project.String() == project.ID &&
				session.Config.Assistant == nil && session.Config.Resource == nil && session.Delegation == nil
		},
		OpenSession: func(cv *viewmodel.Conversation, done func(bool)) {
			root.openProjectSession(project, cv, done)
		},
		Controls: func(view *ui.Context) { root.renderProjectControls(view, project, selectedPath) },
	}
	if side := root.App.Sidebar.Actions; side != nil {
		actions.ResourceActions = side.ProjectResources
		if side.Terminals != nil {
			actions.Terminals = func() []shell.ProjectTerminal { return side.Terminals(project) }
			actions.OpenTerminal = root.showWorkspaceTerminal
		}
	}
	var enteredWorkspace *workspaceProject
	currentWorkspace := func() (*Runtime, *workspaceProject, bool) {
		owner := root.projectOverviewRuntime(project)
		if owner == nil || !owner.controller.Connected() {
			return nil, nil, false
		}
		worktree, ok := owner.projectWorktree(project, selectedPath)
		if !ok {
			return nil, nil, false
		}
		selectedPath = worktree.Path
		workspace := owner.workspaceFor(worktree)
		// Each entry refreshes an unobserved repository once. Subsequent
		// frames reuse it; a new Client's workspace needs its own first read.
		if enteredWorkspace != workspace {
			enteredWorkspace = workspace
			workspace.readGit()
		}
		return owner, workspace, true
	}
	actions.Repository = func() shell.ProjectRepository {
		_, workspace, ok := currentWorkspace()
		if !ok {
			return shell.ProjectRepository{}
		}
		branch := workspaceString(workspace.status.Branch)
		if branch == "" {
			head := workspaceString(workspace.status.Head)
			branch = head[:min(7, len(head))]
		}
		return shell.ProjectRepository{
			Known: workspace.statusKnown, Directory: workspace.status.Kind == protocol.RepositoryKindDirectory,
			Branch: branch, Repositories: append([]string(nil), workspace.status.Repositories...),
		}
	}
	actions.Initialize = func(done func(bool)) {
		_, workspace, ok := currentWorkspace()
		if !ok || !workspace.statusKnown || workspace.status.Kind != protocol.RepositoryKindDirectory {
			root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Conflict, i18n.T(c, "workspace_changed")); done(false) })
			return
		}
		status := workspace.status
		command := protocol.Command{Kind: "run_git_action", Data: protocol.CommandRunGitActionPayload{
			Worktree: workspace.owner.ID, Action: protocol.GitAction{Kind: "initialize"},
			ExpectedIndex: workspaceIndex(status), ExpectedHead: status.Head, ExpectedBranch: status.Branch,
		}}
		workspace.mutateNamed(workspaceAction, command, func(c *ui.Context, out protocol.Output) {
			ok := out.Kind != ""
			if ok {
				workspace.readGit()
				kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
			}
			done(ok)
		})
	}
	currentWorkspace()
	actions.WorktreePath = selectedPath
	root.App.OpenProject(project, actions)
}

func (r *Runtime) openProjectSession(project *viewmodel.Project, cv *viewmodel.Conversation, done func(bool)) {
	owner := r.projectOverviewRuntime(project)
	if owner == nil || cv == nil || cv.Node != project.Host {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")); done(false) })
		return
	}
	session, ok := owner.session(cv.ID)
	if !ok || session.Project == nil || session.Project.String() != project.ID ||
		session.Config.Assistant != nil || session.Config.Resource != nil || session.Delegation != nil {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")); done(false) })
		return
	}
	if !session.Archived {
		owner.OpenSession(cv.ID)
		done(true)
		return
	}
	owner.command(protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{
		Session: session.ID, ExpectedRevision: session.Revision, Archived: false,
	}}, "session_changed", func(_ protocol.Output, ok bool) {
		ok = ok && r.projectOverviewRuntime(project) == owner
		if ok {
			owner.project()
			owner.OpenSession(cv.ID)
		}
		done(ok)
	})
}
