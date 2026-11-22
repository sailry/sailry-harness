package live

import (
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestApprovalRedrawRefreshesAndReleasesDetails(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		inventory, history, details := approvalRedrawFixture(t)
		r, source, cv := childTestRuntime(t, inventory, history)
		source.handle = details
		parent := history.Page.Session
		approval := history.Page.Approvals[0]
		var ctx composer.Context
		locale, show := "en", true
		frame := childTestFrame(r, func(c *ui.Context) {
			r.beginConversationFrame()
			ctx.Interactions.Approvals = nil
			if show {
				r.conversationSeen[parent] = true
				state, _ := r.conversationState(parent)
				for _, current := range state.Snapshot.Page.Approvals {
					if current.State == protocol.ConversationApprovalStatePending {
						ctx.Interactions.Approvals = append(ctx.Interactions.Approvals, projectApproval(current, nil, locale))
					}
				}
				r.projectInteractionStates(cv, &ctx, locale)
			}
			r.finishConversationFrame()
		})
		assertTitle := func(want string) {
			t.Helper()
			if len(ctx.Interactions.Approvals) != 1 || ctx.Interactions.Approvals[0].Title != want || ctx.Interactions.Approvals[0].Disabled {
				t.Fatalf("approval should be actionable with title %q: %+v", want, ctx.Interactions.Approvals)
			}
		}
		frame.Frame()
		if len(ctx.Interactions.Approvals) != 1 || !ctx.Interactions.Approvals[0].Loading {
			t.Fatalf("approval should wait for its first detail: %+v", ctx.Interactions.Approvals)
		}
		synctest.Wait()
		frame.Frame()
		assertTitle("Approval details")
		for range 3 {
			frame.Frame()
		}
		locale = "zh-CN"
		frame.Frame()
		assertTitle("审批详情")
		if len(source.requests) != 1 || source.requests[0].Command.Kind != "read_tool_detail" {
			t.Fatalf("unexpected requests while redrawing approval: %v", source.requests)
		}
		show = false
		frame.Frame()
		if len(r.overviews) != 0 {
			t.Fatal("closing the conversation retained its approval projection")
		}
		show = true
		frame.Frame()
		assertTitle("审批详情")

		approval.State = protocol.ConversationApprovalStateDenied
		source.chat.updates <- protocol.Update{Kind: "conversation_frame", Data: protocol.ConversationFrame{
			Node: history.Node, Session: parent, Sequence: history.Sequence + 1,
			Change: protocol.ConversationChange{Kind: "approval", Data: approval},
		}}
		synctest.Wait()
		frame.Frame()
		if len(ctx.Interactions.Approvals) != 0 || len(r.overviews[parent].approvals) != 0 {
			t.Fatal("a resolved approval remained visible or cached")
		}
		if _, _, ok := r.waitingApproval(cv.ID, approval.ID.String(), r.controller.Conversation); ok {
			t.Fatal("a resolved approval was actionable after redraw")
		}
	})
}
