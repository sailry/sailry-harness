package live

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (p *workspaceProject) bindGitWorkspace() {
	r := p.runtime
	if r == nil || r.App == nil {
		return
	}
	mount := r.App.WorkspaceMount(panel.Changes)
	if mount == nil {
		return
	}
	p.git = mount(p.panel, panel.GitWorkspaceHost{
		Context:  r.ctx,
		Worktree: p.owner.ID,
		Read:     p.readGitWorkspace,
		Observe: func(ctx context.Context, changed func()) (func(), error) {
			return r.controller.ObserveWorkspace(ctx, p.owner.ID, true, func(protocol.Update) {
				r.enqueue(func(*ui.Context) {
					if ctx.Err() == nil {
						changed()
					}
				})
			})
		},
		Post: r.enqueue,
		Mutate: func(command protocol.Command, done func(*ui.Context, protocol.Output, error)) {
			r.executeScoped("workspace-action/"+p.owner.ID.String(), command, func(c *ui.Context, out protocol.Output, err error) {
				if err == nil {
					p.pendingFiles, p.pendingGit = true, true
				}
				done(c, out, err)
			})
		},
		Report: func(c *ui.Context, err error) {
			r.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
		},
		Fault: func(err error) (protocol.ErrorCode, string) { return faultCode(err), faultMessage(err) },
		PublishStatus: func(status protocol.GitStatus, known bool) {
			p.status = protocol.GitStatus{Kind: status.Kind, Branch: status.Branch, Head: status.Head, IndexRevision: status.IndexRevision}
			p.statusKnown, p.repositoryUnavailable = known, !known
		},
		RefreshLocations:        p.invalidateLocations,
		LocationNotice:          func() string { return p.locationWorkspace().readReported["git-worktrees"] },
		PublishInspectionNotice: func(key string) { p.repositoryNotice = key },
		TurnReview:              func() bool { return p.turnView != nil },
		ReadFile:                p.readFile,
		ReadCheckpointHunks:     p.readTurnHunks,
		ApplyCheckpointHunk:     p.applyCheckpointHunk,
		OpenDiff: func(id string) {
			root := r.localRuntime()
			if root.showingResourcePage() {
				if root.shownResourceWorkspace() == p {
					p.showItem(panel.Changes, id)
				}
			} else if root.selectedRuntime().selectedWorkspace() == p {
				p.showItem(panel.Changes, id)
			}
		},
		HasWorkspaceAction: p.hasWorkspaceAction,
		ShowOverlay:        p.showGitOverlay,
		WorkspaceAction:    p.runWorkspaceAction,
		OpenClone:          r.App.Sidebar.OpenCloneEditor,
		Refresh:            p.refresh,
	})
	p.panel.SetGitWorkspace(p.git)
}

func (p *workspaceProject) readGitWorkspace(ctx context.Context, command protocol.Command, done func(*ui.Context, protocol.Output, error)) {
	r := p.runtime
	if r.parent != nil && !r.controller.Connected() {
		r.enqueue(func(c *ui.Context) { done(c, protocol.Output{}, resourceOfflineFault()) })
		return
	}
	request := r.controller.Prepare(command)
	r.async(func() {
		out, err := r.controller.Settle(ctx, request)
		r.enqueue(func(c *ui.Context) {
			if ctx.Err() == nil {
				done(c, out, err)
			}
		})
	})
}

func (p *workspaceProject) invalidateLocations() {
	for _, project := range p.runtime.workspace.projects {
		if sameLocationProject(p.owner, project.owner) {
			project.locationsLoaded = false
		}
	}
}

func (p *workspaceProject) readGit() {
	p.invalidateLocations()
	if p.git.Refresh != nil {
		p.git.Refresh()
	}
}

func (p *workspaceProject) inspectGit() {
	if p.git.Inspect != nil {
		p.git.Inspect()
	}
}

func (p *workspaceProject) resetWorkingChanges() {
	p.data.Changes = nil
	if p.git.ResetChanges != nil {
		p.git.ResetChanges()
	}
}

func workspaceString(value *string) string {
	if value != nil {
		return *value
	}
	return ""
}

func (p *workspaceProject) showGitOverlay(render func(*ui.Context) bool, close func()) {
	r := p.runtime
	app := r.App
	page, target, conversation := app.Page, app.ResourceTarget(), app.Sidebar.Selected
	node, project, overview := app.ProjectOwner()
	app.SetWorkspaceOverlay(func(c *ui.Context) bool {
		n, id, shown := app.ProjectOwner()
		if !r.workflowWindowCurrent() || !r.controller.Connected() || app.DemoOn() ||
			page != app.Page || target != app.ResourceTarget() || conversation != app.Sidebar.Selected ||
			node != n || project != id || overview != shown {
			return false
		}
		return render(c)
	}, close)
}

func (p *workspaceProject) gitFrame() {
	if p.git.Frame == nil {
		return
	}
	p.gitVisible = true
	p.git.Frame(p.pendingGit)
	p.pendingGit = false
}

func (p *workspaceProject) releaseGitView() {
	if p.git.Release != nil {
		p.git.Release()
	}
	p.gitVisible = false
}

func (r *Runtime) gitComposerContext(id string, comp *composer.State, session protocol.Session, p *workspaceProject, editable bool) panel.GitComposerContext {
	current := func() bool {
		return editable && r.controller.Connected() && r.canMoveWorkspace(id, comp) &&
			!comp.Running && r.App.FocusedConversation() == id && r.App.Page == shell.ConversationPage
	}
	draft := session.ID == (protocol.SessionID{})
	ctx := panel.GitComposerContext{Draft: draft, Enabled: current(), Current: current}
	if base := r.pendingSessionBase(comp); base != nil {
		ctx.Base = base.base.From
	}
	if !draft {
		if p.hasWorkspaceAction("worktree_fork_from") {
			ctx.ForkBase = func(c *ui.Context, name, commit string) {
				if current() {
					p.runWorkspaceAction(c, "worktree_fork_from", name, commit)
				}
			}
		}
		return ctx
	}
	ctx.SelectCheckout = func(branch string) bool {
		if !current() {
			return true
		}
		if p.status.Branch != nil && *p.status.Branch == branch {
			delete(r.sessionBases, comp)
			return true
		}
		for _, checkout := range p.locationWorkspace().worktrees.Entries {
			if checkout.Branch == nil || *checkout.Branch != branch || !checkout.Available {
				continue
			}
			if target, ok := p.registeredLocation(checkout.Path); ok {
				delete(r.sessionBases, comp)
				comp.Worktree = target.Path
				return true
			}
		}
		return false
	}
	if r.localRuntime().worktreeActions.Create != nil && session.Project != nil {
		ctx.SelectBase = func(from, branch, commit string) bool {
			if !current() {
				return false
			}
			base := sessionBase{From: from, Branch: branch, Commit: commit}
			if !base.valid() || comp.Project != session.Project.String() || comp.Worktree != p.owner.Path {
				return false
			}
			if r.sessionBases == nil {
				r.sessionBases = make(map[*composer.State]*sessionBaseStart)
			}
			r.sessionBases[comp] = &sessionBaseStart{base: base, project: *session.Project, source: p.owner.ID, worktree: comp.Worktree}
			return true
		}
	}
	return ctx
}
