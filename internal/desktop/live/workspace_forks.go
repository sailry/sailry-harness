package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) forkWorkspaceTurn(x *conversationActions, turn string) {
	if !r.canMoveWorkspace(x.cv, x.comp) {
		return
	}
	session, state, ok := x.state()
	if !ok {
		return
	}
	id, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	found := false
	for _, run := range state.Page.Runs {
		if run.Turn == id && run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusRunning && run.Status != protocol.ConversationStatusStopping {
			found = true
			break
		}
	}
	if !found {
		return
	}
	r.openManagedConversation(x.cv, &session, true, nil)
}
func (r *Runtime) forkWorkspaceConversation(cv *viewmodel.Conversation) {
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.forkWorkspaceConversation(cv)
		return
	}
	session, ok := r.session(cv.ID)
	if !ok {
		return
	}
	r.executeScoped("fork-context/"+session.ID.String(), protocol.Command{Kind: "read_conversation", Data: protocol.CommandReadConversationPayload{Session: session.ID, Limit: 100}}, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			r.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			return
		}
		history, ok := detail[protocol.ConversationHistory](out.Data)
		if !ok {
			return
		}
		if !workspacePageIdle(history.Page) {
			r.reportKeys(c, protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "conversation has active or queued turns"}, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
			return
		}
		r.openManagedConversation(cv, &session, true, nil)
	})
}
func (r *Runtime) openManagedConversation(cv *viewmodel.Conversation, session *protocol.Session, fork bool, selected func(protocol.Worktree)) {
	r.App.Sidebar.Selected = cv.ID
	r.workspaceFrame(nil)
	p := r.workspace.projects[r.workspace.selected]
	if p == nil || p.owner.Project == nil {
		return
	}
	r.App.PanelOpen = true
	r.App.Panel.Show(panel.Tab{Kind: panel.Changes, ID: ":changes"})
	delete(p.failedReads, "managed-worktree-status")
	p.read("managed-worktree-status", protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: p.owner.ID}}, func(c *ui.Context, out protocol.Output) {
		status, ok := detail[protocol.GitStatus](out.Data)
		if !ok {
			return
		}
		if status.Head == nil || status.IndexRevision == nil {
			p.workspaceError(c, protocol.ErrorCodeConflict, "managed worktree requires a committed repository")
			return
		}
		p.managedWorktree(status, session, fork, selected)
	})
}
func (p *workspaceProject) managedWorktree(status protocol.GitStatus, session *protocol.Session, fork bool, selected func(protocol.Worktree)) {
	project := *p.owner.Project
	includeChanges := true
	branch := "sailry/task-" + protocol.NewWorktreeID().String()[:8]
	title := "location_create"
	if fork {
		title = "location_fork"
	}
	var target *protocol.Worktree
	var finish func(func(bool))
	finish = func(done func(bool)) {
		if session == nil {
			if selected != nil {
				selected(*target)
			}
			p.pendingFiles, p.pendingGit = true, true
			done(true)
			return
		}
		command := protocol.Command{Kind: "move_conversation", Data: protocol.CommandMoveConversationPayload{Session: session.ID, Worktree: target.ID, ExpectedRevision: session.Revision}}
		if fork {
			command = protocol.Command{Kind: "fork_conversation_at", Data: protocol.CommandForkConversationAtPayload{Session: session.ID, Worktree: target.ID, ExpectedRevision: session.Revision}}
		}
		p.runtime.executeScoped("managed-conversation/"+session.ID.String(), command, func(c *ui.Context, out protocol.Output, err error) {
			if err != nil {
				p.runtime.reportKeys(c, err, "workspace_action_failed", "workspace_changed", "workspace_action_unknown")
				done(false)
				return
			}
			value, ok := detail[protocol.Session](out.Data)
			if !ok {
				done(false)
				return
			}
			if selected != nil {
				selected(*target)
			}
			p.runtime.project()
			p.runtime.OpenSession(value.ID.String())
			p.pendingFiles, p.pendingGit = true, true
			done(true)
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
		})
	}
	p.panel.Prompt(title, []string{"git_new_branch"}, []string{branch}, func(values []string, done func(bool)) {
		if target != nil {
			finish(done)
			return
		}
		input := protocol.CommandCreateManagedWorktreePayload{Project: project, Source: p.owner.ID, Branch: values[0], ExpectedHead: *status.Head, ExpectedIndex: *status.IndexRevision, IncludeChanges: includeChanges}
		p.runtime.executeScoped("managed-worktree/"+p.owner.ID.String(), protocol.Command{Kind: "create_managed_worktree", Data: input}, func(c *ui.Context, out protocol.Output, err error) {
			if err != nil {
				p.panel.LockPrompt(client.Retained(err))
				outcome, key := workspaceFailure(worktreeCreate, err)
				kit.Toast(c, outcome, i18n.T(c, key))
				done(false)
				return
			}
			value, ok := detail[protocol.Worktree](out.Data)
			if !ok {
				done(false)
				return
			}
			target = &value
			p.runtime.workspace.admitted[value.ID] = value
			p.runtime.workspaceFor(value)
			p.panel.LockPrompt(true)
			finish(done)
			if session == nil {
				kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
			}
		})
	}, panel.PromptToggle{Label: "location_copy_changes", Value: &includeChanges})
}

func (r *Runtime) canMoveWorkspace(cv *viewmodel.Conversation, comp *composer.State) bool {
	session, exists := r.session(cv.ID)
	if !exists {
		return true
	}
	if session.Delegation != nil || len(comp.Attachments) > 0 {
		return false
	}
	state, ok := r.conversationState(session.ID)
	if !ok {
		return false
	}
	return workspacePageIdle(state.Snapshot.Page)
}
func workspacePageIdle(page protocol.ConversationPage) bool {
	if len(page.Queue.Items) > 0 {
		return false
	}
	for _, run := range page.Runs {
		if run.Status == protocol.ConversationStatusQueued || run.Status == protocol.ConversationStatusRunning || run.Status == protocol.ConversationStatusStopping {
			return false
		}
	}
	return true
}
func (r *Runtime) selectConversationWorktree(cv *viewmodel.Conversation, comp *composer.State, relative string) {
	session, ok := r.session(cv.ID)
	if !ok {
		return
	}
	if !r.canMoveWorkspace(cv, comp) {
		return
	}
	p := r.workspace.projects[session.Worktree]
	if p == nil || session.Project == nil {
		return
	}
	comp.Worktree = p.owner.Path
	move := func(tree protocol.Worktree) {
		if tree.ID == session.Worktree {
			return
		}
		p.mutate(protocol.Command{Kind: "move_conversation", Data: protocol.CommandMoveConversationPayload{Session: session.ID, Worktree: tree.ID, ExpectedRevision: session.Revision}}, func(c *ui.Context, out protocol.Output) {
			if _, ok := detail[protocol.Session](out.Data); ok {
				comp.References = nil
				r.project()
				r.OpenSession(cv.ID)
				kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
			}
		})
	}
	for _, tree := range r.snapshot().Worktrees {
		if tree.Path == relative && tree.Project != nil && *tree.Project == *session.Project {
			move(tree)
			return
		}
	}
	p.mutateNamed(worktreeOpen, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: *session.Project, Path: relative}}, func(c *ui.Context, out protocol.Output) {
		if tree, ok := detail[protocol.Worktree](out.Data); ok {
			r.workspace.admitted[tree.ID] = tree
			r.workspaceFor(tree)
			move(tree)
		}
	})
}
