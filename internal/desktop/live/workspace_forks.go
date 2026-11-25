package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) forkWorkspaceTurn(x *conversationActions, turn string) {
	fork := r.localRuntime().worktreeActions.ForkTurn
	if fork == nil || !r.canMoveWorkspace(x.cv.ID, x.comp) {
		return
	}
	_, state, ok := x.state()
	if !ok {
		return
	}
	id, err := protocol.ParseTurnID(turn)
	if err != nil {
		return
	}
	for _, run := range state.Page.Runs {
		if run.Turn == id && run.Status != protocol.ConversationStatusQueued && run.Status != protocol.ConversationStatusRunning && run.Status != protocol.ConversationStatusStopping {
			fork(r.workflowConversation(x.cv, x.comp))
			return
		}
	}
}
func (r *Runtime) forkWorkspaceConversation(cv *viewmodel.Conversation) {
	owner := r.conversationRuntime(cv, nil)
	if owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	}
	if fork := r.localRuntime().worktreeActions.ForkSession; fork != nil {
		fork(owner.workflowConversation(cv, nil))
	}
}

func (r *Runtime) canMoveWorkspace(id string, comp *composer.State) bool {
	session, exists := r.session(id)
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
func (r *Runtime) selectConversationWorktree(id string, comp *composer.State, relative string) {
	session, ok := r.session(id)
	if !ok {
		return
	}
	if !r.canMoveWorkspace(id, comp) {
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
		p.mutateNamed(workspaceAction, protocol.Command{Kind: "move_conversation", Data: protocol.CommandMoveConversationPayload{Session: session.ID, Worktree: tree.ID, ExpectedRevision: session.Revision}}, func(c *ui.Context, out protocol.Output) {
			if _, ok := detail[protocol.Session](out.Data); ok {
				comp.References = nil
				r.project()
				r.OpenSession(id)
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
