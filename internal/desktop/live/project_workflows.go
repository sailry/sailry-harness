package live

import (
	"github.com/egoist/mygo/ui"
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
	r.workspaceActions = make(map[string]shell.WorkspaceAction)
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
		RegisterWorkspaceAction: func(id string, run shell.WorkspaceAction) { r.workspaceActions[id] = run },
		NewDraft:                r.workflowDraft,
		ProjectWorkspace:        r.workflowProjectWorkspace,
	}
	for _, bind := range workflows {
		bind(host)
	}
}

func (p *workspaceProject) hasWorkspaceAction(id string) bool {
	return p.runtime.localRuntime().workspaceActions[id] != nil
}

func (p *workspaceProject) runWorkspaceAction(c *ui.Context, id, target, secondary string) {
	r := p.runtime
	root := r.localRuntime()
	action := root.workspaceActions[id]
	if action == nil {
		return
	}
	if root.shownResourceWorkspace() == p {
		action(c, p.workflowResourceWorkspace(), target, secondary)
	} else if cv := r.App.SelectedConversation(); cv != nil && r.selectedWorkspace() == p {
		comp := r.App.Composer(cv.ID)
		focused := r.App.FocusedConversation()
		if r.conversationRuntime(cv, comp) != r || focused != "" && focused != cv.ID {
			return
		}
		workspace := p.workflowConversationWorkspace(cv.ID, comp)
		if focused != "" {
			current := workspace.Current
			workspace.Current = func() bool { return current() && r.App.FocusedConversation() == focused }
		}
		action(c, workspace, target, secondary)
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
			if !r.workflowWindowCurrent() {
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
			if comp == nil {
				comp = r.App.Composer(cv.ID)
			}
			return p.workflowConversationWorkspace(cv.ID, comp), true
		},
	}
}

func (r *Runtime) workflowWindowCurrent() bool {
	return r.ctx.Err() == nil && r.localRuntime().runtimeFor(r.controller.Target().String()) == r
}

func (r *Runtime) workflowProjectWorkspace(project *viewmodel.Project) (shell.WorkflowWorkspace, bool) {
	owner := r.workflowProjectOwner(project)
	if owner == nil {
		return shell.WorkflowWorkspace{}, false
	}
	tree, ok := owner.projectWorktree(project, "")
	if !ok {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the project workspace is unavailable"))
		return shell.WorkflowWorkspace{}, false
	}
	p := owner.workspaceFor(tree)
	page, selected := r.App.Page, r.App.Sidebar.Selected
	node, projectID, overview := r.App.ProjectOwner()
	current := func() bool {
		currentNode, currentProject, currentOverview := r.App.ProjectOwner()
		return owner.workflowWindowCurrent() && r.projectOverviewRuntime(project) == owner && r.App.Page == page &&
			r.App.Sidebar.Selected == selected && currentNode == node && currentProject == projectID && currentOverview == overview
	}
	workspace := p.workflowWorkspace(current, func(_ *ui.Context, value protocol.Worktree) {
		owner.openProjectOverviewAt(project, value.Path)
	})
	workspace.CanSelectLocation = current
	return workspace, true
}

func (p *workspaceProject) workflowConversationWorkspace(id string, comp *composer.State) shell.WorkflowWorkspace {
	r := p.runtime
	root := r.localRuntime()
	path := comp.Worktree
	viewCurrent := func() bool {
		return r.workflowWindowCurrent() && !root.showingResourcePage() && r.App.Sidebar.Selected == id
	}
	current := func() bool {
		if !viewCurrent() || r.selectedWorkspace() != p || comp.Worktree != path {
			return false
		}
		if session, exists := r.session(id); exists {
			return session.Worktree == p.owner.ID
		}
		return p.owner.Project == nil || comp.Project == p.owner.Project.String()
	}
	use := func(tree protocol.Worktree) {
		if sameLocationProject(p.owner, tree) {
			comp.Worktree, comp.References = tree.Path, nil
		}
	}
	workspace := p.workflowWorkspace(current, func(_ *ui.Context, tree protocol.Worktree) {
		if session, exists := r.session(id); exists {
			r.moveConversationLocation(session, comp, tree)
		} else {
			use(tree)
		}
	})
	if session, exists := r.session(id); exists {
		workspace.Session = &session
	}
	workspace.Prompt = func(title string, labels, values []string, submit func([]string, func(bool)), toggles ...panel.PromptToggle) {
		r.App.RevealConversationPanel()
		if len(r.App.Panel.Tabs) == 0 {
			r.App.Panel.Show(panel.Tab{Kind: panel.Changes, ID: ":changes"})
		}
		p.panel.Prompt(title, labels, values, submit, toggles...)
	}
	workspace.CanSelectLocation = func() bool {
		if !current() || comp.Running || !r.canMoveWorkspace(id, comp) {
			return false
		}
		cv := r.App.Data.Find(id)
		return cv == nil || !unsentDraft(cv) || !r.draftSubmitted(cv)
	}
	workspace.UseLocation = func(tree protocol.Worktree) {
		if viewCurrent() && comp.Worktree == path {
			use(tree)
		}
	}
	workspace.OpenSession = func(id protocol.SessionID) {
		if viewCurrent() {
			r.project()
			r.OpenSession(id.String())
		}
	}
	return workspace
}

func (p *workspaceProject) workflowResourceWorkspace() shell.WorkflowWorkspace {
	owner := p.runtime
	root := owner.localRuntime()
	page, target := root.App.Page, root.App.ResourceTarget()
	current := func() bool {
		return owner.workflowWindowCurrent() && root.App.Page == page && root.App.ResourceTarget() == target && root.shownResourceWorkspace() == p
	}
	workspace := p.workflowWorkspace(current, func(_ *ui.Context, tree protocol.Worktree) {
		root.App.SetResourceTarget(shell.ResourceTarget{Node: owner.controller.Target().String(), Project: tree.Project.String(), Worktree: tree.ID.String()})
	})
	workspace.CanSelectLocation = current
	return workspace
}

func (p *workspaceProject) workflowWorkspace(current func() bool, selectLocation func(*ui.Context, protocol.Worktree)) shell.WorkflowWorkspace {
	r := p.runtime
	refreshLocations := func() {
		for _, workspace := range r.workspace.projects {
			if sameLocationProject(workspace.owner, p.owner) {
				workspace.locationsLoaded = false
			}
		}
	}
	return shell.WorkflowWorkspace{
		Worktree:     p.owner,
		Presentation: p.panel,
		Current:      current,
		Read: func(key string, command protocol.Command, done func(*ui.Context, protocol.Output)) {
			delete(p.failedReads, key)
			p.read(key, command, func(c *ui.Context, out protocol.Output) {
				if current() {
					done(c, out)
				}
			})
		},
		Execute: func(scope string, command protocol.Command, done func(*ui.Context, protocol.Output, error)) {
			r.executeScoped("workspace-workflow/"+p.owner.ID.String()+"/"+scope, command, done)
		},
		Report: r.reportKeys,
		Pick: func(title string, rows []panel.PickerRow) {
			if current() {
				show := panel.WorkspacePicker(title, rows)
				r.App.SetWorkspaceOverlay(func(c *ui.Context) bool { return current() && show(c) }, nil)
			}
		},
		Prompt:             p.panel.Prompt,
		Confirm:            p.panel.Confirm,
		LockPrompt:         p.panel.LockPrompt,
		SetPromptAction:    p.panel.SetPromptAction,
		SetPromptDetail:    p.panel.SetPromptDetail,
		SetPromptUncertain: p.panel.SetPromptUncertain,
		Locations:          p.workflowLocations,
		HasDrafts: func(id protocol.WorktreeID) bool {
			workspace := r.workspace.projects[id]
			return workspace != nil && sameLocationProject(p.owner, workspace.owner) && workspace.hasFileDrafts()
		},
		Admit: func(tree protocol.Worktree) {
			if sameLocationProject(p.owner, tree) {
				r.workspace.admitted[tree.ID] = tree
				r.workspaceFor(tree)
				refreshLocations()
			}
		},
		Release: func(id protocol.WorktreeID) {
			if workspace := r.workspace.projects[id]; workspace != nil && sameLocationProject(p.owner, workspace.owner) {
				r.releaseWorkspacePresentation(id)
			} else if tree, ok := r.workspace.admitted[id]; ok && sameLocationProject(p.owner, tree) {
				r.releaseWorkspacePresentation(id)
			}
			refreshLocations()
		},
		SelectLocation: func(c *ui.Context, tree protocol.Worktree) {
			if current() && sameLocationProject(p.owner, tree) {
				selectLocation(c, tree)
			}
		},
		Refresh:          func() { p.pendingFiles, p.pendingGit = true, true },
		RefreshLocations: refreshLocations,
	}
}

func sameLocationProject(source, target protocol.Worktree) bool {
	return source.Project != nil && target.Project != nil && *source.Project == *target.Project
}

func (p *workspaceProject) registeredLocation(path string) (protocol.Worktree, bool) {
	for _, tree := range p.runtime.snapshot().Worktrees {
		if tree.Path == path && sameLocationProject(p.owner, tree) {
			return tree, true
		}
	}
	for _, tree := range p.runtime.workspace.admitted {
		if tree.Path == path && sameLocationProject(p.owner, tree) {
			return tree, true
		}
	}
	return protocol.Worktree{}, false
}

func (p *workspaceProject) hasFileDrafts() bool {
	for _, path := range p.panel.Files.Open() {
		if _, dirty := p.panel.FileDraft(path); dirty {
			return true
		}
	}
	return false
}

func (p *workspaceProject) workflowLocations() ([]protocol.Worktree, []protocol.Session) {
	state := p.runtime.snapshot()
	var worktrees []protocol.Worktree
	seen := make(map[protocol.WorktreeID]bool)
	for _, tree := range state.Worktrees {
		if sameLocationProject(p.owner, tree) {
			worktrees = append(worktrees, tree)
			seen[tree.ID] = true
		}
	}
	for _, tree := range p.runtime.workspace.admitted {
		if !seen[tree.ID] && sameLocationProject(p.owner, tree) {
			worktrees = append(worktrees, tree)
			seen[tree.ID] = true
		}
	}
	var sessions []protocol.Session
	for _, session := range state.Sessions {
		if seen[session.Worktree] {
			sessions = append(sessions, session)
		}
	}
	return worktrees, sessions
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
