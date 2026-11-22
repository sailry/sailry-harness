package live

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func BenchmarkDelegationRedraw(b *testing.B) {
	b.Run("Children", func(b *testing.B) {
		node, history := liveChildFixture()
		for i := 3; i <= 129; i++ {
			entry := history.Page.Entries[0]
			entry.ID, entry.Sequence = fmt.Sprintf("spawn-%d", i), uint64(i-1)
			child, session := history.Page.Children[0], node.Sessions[1]
			child.Run.Session, child.Run.Turn, child.Run.Sequence = protocol.SessionID{byte(i)}, protocol.TurnID{byte(i)}, uint64(i)
			child.Run.Status, child.Origin.Entry = protocol.ConversationStatusCompleted, entry.ID
			session.ID, session.Delegation = child.Run.Session, new(new(child.Origin))
			node.Sessions = append(node.Sessions, session)
			history.Page.Entries = append(history.Page.Entries, entry)
			history.Page.Children = append(history.Page.Children, child)
		}
		r, cv := delegationRedrawRuntime(b, &conversationTestTransport{snapshot: node, conversation: history})
		block := conversation.Block{}
		for _, child := range history.Page.Children {
			block.Children = append(block.Children, transcriptChild(&history.Page, child, "en"))
		}
		cv.Turns = []conversation.Turn{{Reply: conversation.Reply{Blocks: []conversation.Block{block}}}}
		var activity composer.Context
		var overview *composer.Overview
		frame := ui.NewTester(func(c *ui.Context) {
			r.beginConversationFrame()
			r.conversationSeen[history.Page.Session] = true
			r.projectChildComposer(cv, r.App.Composer(cv.ID), &activity, c.Locale())
			overview = r.sessionOverview(c, cv)
			r.finishConversationFrame()
		}, 520, 400)
		frame.Frame()
		if len(activity.Activity.Children) != 1 || len(overview.Children) != 1 {
			b.Fatal("the fixture should display one running child")
		}
		b.ReportAllocs()
		for b.Loop() {
			frame.Frame()
		}
	})
	b.Run("Approval", func(b *testing.B) {
		node, history, details := approvalRedrawFixture(b)
		source := &conversationTestTransport{snapshot: node, conversation: history, handle: details}
		r, cv := delegationRedrawRuntime(b, source)
		call := history.Page.Entries[0].Parts[0].Data.(protocol.ConversationPartToolCallPayload)
		approval := history.Page.Approvals[0]
		ref := protocol.ToolDetailReference{Session: approval.Session, Turn: approval.Turn, Entry: approval.Entry, Revision: call.Summary.Revision}
		if _, err := r.controller.ReadConversationDetail(b.Context(), ref); err != nil {
			b.Fatal(err)
		}
		var ctx composer.Context
		listed := []composer.Approval{{ID: approval.ID.String(), Title: call.Name}}
		frame := ui.NewTester(func(c *ui.Context) {
			r.beginConversationFrame()
			r.conversationSeen[history.Page.Session] = true
			ctx.Interactions.Approvals = listed
			r.projectInteractionStates(cv, &ctx, c.Locale())
			r.finishConversationFrame()
		}, 520, 400)
		frame.Frame()
		if len(ctx.Interactions.Approvals) != 1 || ctx.Interactions.Approvals[0].Title != "Approval details" {
			b.Fatalf("the fixture should display loaded approval details: %+v", ctx.Interactions.Approvals)
		}
		b.ReportAllocs()
		for b.Loop() {
			frame.Frame()
		}
	})
}

func approvalRedrawFixture(t testing.TB) (protocol.Snapshot, protocol.ConversationSnapshot, func(protocol.Request) (link.Response, bool)) {
	t.Helper()
	node, history := liveChildFixture()
	history.Page.Children = nil
	entry := &history.Page.Entries[0]
	approval := protocol.ConversationApproval{ID: protocol.ApprovalID{1}, Session: history.Page.Session, Turn: entry.Turn,
		Entry: entry.ID, State: protocol.ConversationApprovalStatePending, Source: protocol.ConversationApprovalSourceUser}
	history.Page.Approvals = []protocol.ConversationApproval{approval}
	ref := protocol.ToolDetailReference{Session: approval.Session, Turn: approval.Turn, Entry: entry.ID, Revision: "detail-revision"}
	call := entry.Parts[0].Data.(protocol.ConversationPartToolCallPayload)
	call.Summary.HasDetails, call.Summary.Revision = true, ref.Revision
	body := protocol.DefaultToolDetail()
	body.Arguments = json.RawMessage(`{"text":"` + strings.Repeat("fixture ", 4096) + `"}`)
	body.Resolved = &protocol.ToolDetailResolved{Label: &protocol.ToolResultLocalText{Label: "Approval details", Locales: map[string]string{"zh-CN": "审批详情"}}}
	encoded, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	return node, history, func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind != "read_tool_detail" {
			return link.Response{}, false
		}
		return link.Response{Output: protocol.Output{Kind: "tool_detail", Data: protocol.ToolDetailPage{
			Reference: ref, TotalBytes: uint64(len(encoded)), Text: string(encoded),
		}}}, true
	}
}

func delegationRedrawRuntime(b *testing.B, source *conversationTestTransport) (*Runtime, *viewmodel.Conversation) {
	b.Helper()
	controller := client.New(source)
	b.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(b.Context()); err != nil {
		b.Fatal(err)
	}
	parent := source.conversation.Page.Session
	changed := make(chan struct{}, 1)
	release, err := controller.WatchConversation(b.Context(), parent, func() {
		select {
		case changed <- struct{}{}:
		default:
		}
	})
	if err != nil {
		b.Fatal(err)
	}
	b.Cleanup(release)
	var state client.ConversationState
	for {
		state, _ = controller.ConversationView(parent, state)
		if state.Err != nil {
			b.Fatal(state.Err)
		}
		if state.Connected && state.Snapshot.Page.Revision != 0 {
			break
		}
		<-changed
	}
	cv := &viewmodel.Conversation{Node: source.snapshot.Node.String(), ID: parent.String()}
	app := shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{cv}})
	r := &Runtime{App: app, ctx: b.Context(), controller: controller, teams: newTeamState(),
		conversationCache: map[protocol.SessionID]conversationCopy{parent: {state: state, ok: true, generation: 1}}}
	return r, cv
}
