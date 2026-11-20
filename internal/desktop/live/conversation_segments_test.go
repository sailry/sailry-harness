package live

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConversationGuidanceKeepsRecordedPosition(t *testing.T) {
	owner, guide := protocol.TurnID{1}, protocol.TurnID{2}
	file := protocol.Attachment{ID: protocol.AttachmentID{3}, Spec: protocol.AttachmentSpec{Name: "constraint.txt", MediaType: "text/plain"}}
	picture := protocol.Attachment{ID: protocol.AttachmentID{4}, Spec: protocol.AttachmentSpec{Name: "constraint.png", MediaType: "image/png"}}
	page := protocol.DefaultConversationPage()
	page.Runs = []protocol.ConversationRun{
		{Turn: owner, Sequence: 1, Status: "running"},
		{Turn: guide, Sequence: 9, Status: "completed", Joined: &owner, Guidance: &owner},
	}
	page.Entries = []protocol.ConversationEntry{
		{ID: "initial", Sequence: 1, Turn: owner, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Initial request"}}},
		{ID: "before", Sequence: 2, Turn: owner, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Earlier answer"}}},
		{ID: "guidance", Sequence: 3, Turn: guide, Author: "user", Parts: []protocol.ConversationPart{
			{Kind: "text", Data: "Inserted constraint"}, {Kind: "attachment", Data: file}, {Kind: "image", Data: protocol.ConversationImage{Attachment: picture}},
		}},
	}
	state := client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page,
		Drafts: []protocol.ConversationDraft{{ID: "draft", Turn: owner, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Later answer"}}}}}}
	// The queue update that removes the consumed guidance arrives after its
	// entry: the stale row never shows beside the transcript's copy.
	state.Snapshot.Page.Runs[1].Status = "queued"
	state.Snapshot.Page.Queue.Items = []protocol.ConversationPending{{Turn: guide, Kind: "task", Preview: "Inserted constraint", Guidance: &owner}}
	if _, _, _, queue := Conversation(state, "en"); len(queue) != 0 {
		t.Fatalf("consumed guidance stayed queued: %+v", queue)
	}
	state.Snapshot.Page.Runs[1].Status = "completed"
	state.Snapshot.Page.Queue.Items = nil
	turns, _, _, _ := Conversation(state, "en")
	if len(turns) != 1 || turns[0].ID != owner.String() || len(turns[0].Segments) != 2 {
		t.Fatalf("guidance changed the task identity or lost its boundary: %+v", turns)
	}
	turn := &turns[0]
	if turn.Segments[0].From != 0 || turn.Segments[1].From != 1 || len(turn.Reply.Blocks) != 2 ||
		turn.Reply.Blocks[0].Text != "Earlier answer" || turn.Reply.Blocks[1].Text != "Later answer" {
		t.Fatalf("assistant text crossed an input boundary: %+v", turn)
	}
	initial, inserted := turn.Segments[0].Inputs[0], turn.Segments[1].Inputs[0]
	if initial.ID != owner.String() || initial.User != "Initial request" || len(initial.Files)+len(initial.Images) != 0 ||
		inserted.ID != guide.String() || inserted.User != "Inserted constraint" || !slices.Equal(inserted.Files, []int{0}) || !slices.Equal(inserted.Images, []int{0}) ||
		turn.Files[0].ID != file.ID.String() || turn.Images[0].ID != picture.ID.String() {
		t.Fatalf("input identity or attachment ownership changed: %+v", turn.Segments)
	}
	// Asset normalization must keep segment indices valid after enrichment.
	sentAttachmentImages(turn)
	view := conversation.New()
	tester := ui.NewTesterWith(func(c *ui.Context) {
		kit.SetPreferences(c, kit.Preferences{})
		ui.Column(c).Fill().Children(func() { conversation.View(c, view, "session", turns, &conversation.Session{}, referenceViewActions{}) })
	}, 900, 1100, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	tester.Frame()
	tester.Frame()
	var bottom float32
	for _, text := range []string{"Initial request", "Earlier answer", "Inserted constraint", "Later answer"} {
		bounds, ok := tester.Find(text)
		if !ok || bounds.Y < bottom {
			t.Fatalf("%q is missing or before the previous exchange: %q", text, tester.Texts())
		}
		bottom = bounds.Y + bounds.H
	}
	if tester.HasText("Initial request\n\nInserted constraint") {
		t.Fatal("guidance was displayed inside the original bubble")
	}
	if len(conversation.Find(turns, "answer")) != 2 {
		t.Fatal("search lost an earlier exchange")
	}
	// There is one live task status, not a new task for each guidance input.
	statuses := 0
	for _, text := range tester.Texts() {
		if text == i18n.In("en", "chat_running") {
			statuses++
		}
	}
	if statuses != 1 {
		t.Fatalf("the task has %d live status headers", statuses)
	}
}

func TestConversationToolResultAcrossGuidanceKeepsItsCall(t *testing.T) {
	owner, guide := protocol.TurnID{1}, protocol.TurnID{2}
	callID := "read-before-guidance"
	page := protocol.DefaultConversationPage()
	page.Runs = []protocol.ConversationRun{{Turn: owner, Status: "running"}, {Turn: guide, Status: "completed", Joined: &owner}}
	page.Entries = []protocol.ConversationEntry{
		{ID: "initial", Sequence: 1, Turn: owner, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Inspect"}}},
		{ID: "call", Sequence: 2, Turn: owner, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{ID: &callID, Name: "read_file"}}}},
		{ID: "guide", Sequence: 3, Turn: guide, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Also check this"}}},
		{ID: "result", Sequence: 4, Turn: owner, Author: "tool", Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{ID: &callID, Name: "read_file", Summary: &protocol.ToolDetailSummary{}}}}},
		{ID: "answer", Sequence: 5, Turn: owner, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Done"}}},
	}
	turns, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}, "en")
	turn := turns[0]
	if len(turn.Segments) != 2 || turn.Segments[1].From != 1 || len(turn.Reply.Blocks) != 2 ||
		len(turn.Reply.Blocks[0].Calls) != 1 || turn.Reply.Blocks[0].Calls[0].ID != toolCallKey("call", 0) ||
		turn.Reply.Blocks[0].Calls[0].Running || turn.Reply.Blocks[1].Text != "Done" {
		t.Fatalf("guidance duplicated or detached the completed tool: %+v", turn)
	}
}
