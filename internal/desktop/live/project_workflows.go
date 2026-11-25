package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) bindProjectWorkflows(workflows []shell.ProjectWorkflow) {
	if len(workflows) == 0 {
		return
	}
	host := shell.ProjectWorkflowHost{
		RegisterMenu:      r.App.Sidebar.Actions.RegisterProjectMenu,
		RegisterLocations: func(read func(shell.ProjectLocationReader)) { r.projectLocations = read },
		RegisterWorktreeActions: func(actions shell.ConversationWorktreeActions) {
			r.worktreeActions = actions
			if actions.ForkSession != nil {
				r.App.Sidebar.Actions.ForkWorktree = r.forkWorkspaceConversation
			} else {
				r.App.Sidebar.Actions.ForkWorktree = nil
			}
		},
		NewDraft: r.workflowDraft,
		OpenLocation: func(project *viewmodel.Project, path string) {
			if owner := r.workflowProjectOwner(project); owner != nil {
				owner.openProjectWorktree(project, path)
			}
		},
	}
	for _, bind := range workflows {
		bind(host)
	}
}

func (r *Runtime) workflowProjectOwner(project *viewmodel.Project) *Runtime {
	if project == nil {
		return nil
	}
	owner := r.projectOverviewRuntime(project)
	if owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
	}
	return owner
}

func (r *Runtime) workflowDraft(project *viewmodel.Project) (shell.WorkflowConversation, bool) {
	owner := r.workflowProjectOwner(project)
	if owner == nil {
		return shell.WorkflowConversation{}, false
	}
	r.localRuntime().newDraft(project)
	cv := r.App.SelectedConversation()
	if cv == nil {
		return shell.WorkflowConversation{}, false
	}
	comp := r.App.Composer(cv.ID)
	comp.Project = project.ID
	return owner.workflowConversation(cv, comp), true
}

func (r *Runtime) workflowConversation(cv *viewmodel.Conversation, comp *composer.State) shell.WorkflowConversation {
	var session *protocol.Session
	if value, ok := r.session(cv.ID); ok {
		session = &value
	}
	return shell.WorkflowConversation{
		Session: session,
		Execute: func(scope string, command protocol.Command, done func(*ui.Context, protocol.Output, error)) {
			r.executeScoped(scope, command, done)
		},
		Report: r.reportKeys,
		OpenWorkspace: func() (shell.WorkflowWorkspace, bool) {
			if r.ctx.Err() != nil || r.localRuntime().runtimeFor(r.controller.Target().String()) != r {
				return shell.WorkflowWorkspace{}, false
			}
			r.App.Sidebar.Selected = cv.ID
			r.workspaceFrame(nil)
			p := r.workspace.projects[r.workspace.selected]
			if p == nil || p.owner.Project == nil {
				return shell.WorkflowWorkspace{}, false
			}
			r.App.RevealConversationPanel()
			r.App.Panel.Show(panel.Tab{Kind: panel.Changes, ID: ":changes"})
			return p.workflowWorkspace(comp), true
		},
	}
}

func (p *workspaceProject) workflowWorkspace(comp *composer.State) shell.WorkflowWorkspace {
	r := p.runtime
	return shell.WorkflowWorkspace{
		Worktree: p.owner,
		Read: func(key string, command protocol.Command, done func(*ui.Context, protocol.Output)) {
			delete(p.failedReads, key)
			p.read(key, command, done)
		},
		Prompt:     p.panel.Prompt,
		LockPrompt: p.panel.LockPrompt,
		ReportMutation: func(c *ui.Context, command, action string, err error) bool {
			reportWorkspaceMutation(c, command, action, err)
			return client.Retained(err)
		},
		Admit: func(tree protocol.Worktree) {
			r.workspace.admitted[tree.ID] = tree
			r.workspaceFor(tree)
		},
		UseLocation: func(tree protocol.Worktree) {
			if comp != nil {
				comp.Worktree = tree.Path
			}
			p.locationsLoaded = false
		},
		OpenSession: func(id protocol.SessionID) {
			r.project()
			r.OpenSession(id.String())
		},
		Refresh:         func() { p.pendingFiles, p.pendingGit = true, true },
		ManageLocations: p.manageWorktrees,
	}
}

func (p *workspaceProject) projectLocationReader() shell.ProjectLocationReader {
	return shell.ProjectLocationReader{
		Main: p.owner.ID,
		Read: func(key string, command protocol.Command, done func(protocol.Output)) {
			p.read(key, command, func(_ *ui.Context, out protocol.Output) { done(out) })
		},
		Publish: func(value protocol.GitWorktrees) {
			p.worktrees = value
			p.runtime.projectWorkspaceLocations()
		},
	}
}
