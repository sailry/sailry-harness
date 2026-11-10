package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) exportSession(cv *viewmodel.Conversation, format conversation.ExportFormat) {
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.exportSession(cv, format)
		return
	}
	if r.exportConversation != nil {
		r.exportConversation(cv.ID, "", format)
	} else {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "unavailable")) })
	}
}

func (r *Runtime) forkHere(cv *viewmodel.Conversation) {
	if owner := r.conversationRuntime(cv, nil); owner == nil {
		r.Report(fileActionFault(protocol.ErrorCodeUnavailable, "the execution host is unavailable"))
		return
	} else if owner != r {
		owner.forkHere(cv)
		return
	}
	s, ok := r.session(cv.ID)
	if !ok || s.Project == nil {
		return
	}
	r.command(protocol.Command{Kind: "fork_conversation_at", Data: protocol.CommandForkConversationAtPayload{Session: s.ID, Worktree: s.Worktree, ExpectedRevision: cv.Revision}}, "session_changed", func(output protocol.Output, ok bool) {
		if !ok {
			return
		}
		if fork, yes := detail[protocol.Session](output.Data); yes {
			r.admittedSessions[fork.ID.String()] = fork
			r.project()
			r.OpenSession(fork.ID.String())
		}
	})
}
