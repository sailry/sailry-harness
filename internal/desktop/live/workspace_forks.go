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
func (r *Runtime) selectConversationLocation(id string, comp *composer.State, path string) {
	session, ok := r.session(id)
	if !ok {
		return
	}
	if p := r.workspace.projects[session.Worktree]; p != nil {
		if tree, found := p.registeredLocation(path); found {
			r.moveConversationLocation(session, comp, tree)
		}
	}
}

func (r *Runtime) moveConversationLocation(session protocol.Session, comp *composer.State, tree protocol.Worktree) {
	id := session.ID.String()
	p := r.workspace.projects[session.Worktree]
	if p == nil || tree.ID == session.Worktree || !sameLocationProject(p.owner, tree) || !r.canMoveWorkspace(id, comp) {
		return
	}
	p.mutateNamed(workspaceAction, protocol.Command{Kind: "move_conversation", Data: protocol.CommandMoveConversationPayload{Session: session.ID, Worktree: tree.ID, ExpectedRevision: session.Revision}}, func(c *ui.Context, out protocol.Output) {
		if _, ok := detail[protocol.Session](out.Data); ok {
			comp.References = nil
			r.project()
			if r.App.Sidebar.Selected == id && !r.localRuntime().showingResourcePage() {
				r.OpenSession(id)
			}
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
		}
	})
}
