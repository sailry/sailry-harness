package live

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestContinuationMarker covers a turn a package continued: the transcript
// shows the package's marker in place of the prompt the package sent the
// model, guidance the user joined into the turn still shows as theirs, and a
// queued continuation is an automatic queue row named by the marker.
func TestContinuationMarker(t *testing.T) {
	session := protocol.SessionID{1}
	first, continued, guidance, queued := protocol.TurnID{1}, protocol.TurnID{2}, protocol.TurnID{3}, protocol.TurnID{4}
	marker := &protocol.PluginDesktopNavigation{Label: "Continue goal", Locales: map[string]string{"zh-CN": "继续执行目标"}}
	started := func(ms int64) *int64 { return &ms }
	prompt := "Continue working toward the goal.\nThis is automatic turn 1 of at most 25 since the user's last message."
	text := func(id string, turn protocol.TurnID, author, body string) protocol.ConversationEntry {
		return protocol.ConversationEntry{ID: id, Turn: turn, Author: author, Parts: []protocol.ConversationPart{{Kind: "text", Data: body}}}
	}
	page := protocol.DefaultConversationPage()
	page.Session = session
	page.Runs = []protocol.ConversationRun{
		{Session: session, Turn: first, Sequence: 1, Kind: "task", Status: "completed", StartedMs: started(1)},
		{Session: session, Turn: continued, Sequence: 2, Kind: "task", Status: "completed", StartedMs: started(2),
			Continuation: &protocol.ConversationContinuation{Marker: marker},
			Input:        &protocol.ConversationSentInput{Message: protocol.ConversationInput{Text: prompt}}},
		{Session: session, Turn: guidance, Sequence: 3, Kind: "task", Status: "completed", StartedMs: started(3), Joined: &continued},
		{Session: session, Turn: queued, Sequence: 4, Kind: "task", Status: "queued", Continuation: &protocol.ConversationContinuation{}},
	}
	page.Entries = []protocol.ConversationEntry{
		text("ask", first, "user", "Ship the feature"),
		text("answer", first, "assistant", "Started"),
		text("prompt", continued, "user", prompt),
		text("guide", guidance, "user", "Also update the docs"),
		text("work", continued, "assistant", "Step done"),
	}
	page.Queue.Items = []protocol.ConversationPending{{Turn: queued, Kind: "task", Ready: true, Preview: prompt}}
	turns, _, _, queue := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}}, "zh-CN")
	if len(turns) != 2 {
		t.Fatalf("the transcript has %d turns: %+v", len(turns), turns)
	}
	t.Run("the user's turn is unmarked", func(t *testing.T) {
		if turns[0].Marker != nil || turns[0].User != "Ship the feature" {
			t.Fatalf("user turn = %+v", turns[0])
		}
	})
	t.Run("the continued turn shows the marker and not its prompt", func(t *testing.T) {
		turn := turns[1]
		if turn.Marker == nil || turn.Marker.Label != "Continue goal" || turn.Marker.Locales["zh-CN"] != "继续执行目标" {
			t.Fatalf("marker = %+v", turn.Marker)
		}
		// Only the guidance the user joined into the turn is a user message.
		if turn.User != "Also update the docs" {
			t.Fatalf("the continued turn shows %q as the user's message", turn.User)
		}
		if len(turn.Reply.Blocks) != 1 || turn.Reply.Blocks[0].Text != "Step done" {
			t.Fatalf("reply = %+v", turn.Reply.Blocks)
		}
	})
	t.Run("a continuation without guidance has no user message", func(t *testing.T) {
		bare := page
		bare.Runs = page.Runs[:2]
		bare.Entries = []protocol.ConversationEntry{page.Entries[0], page.Entries[1], page.Entries[2], page.Entries[4]}
		bare.Queue.Items = nil
		turns, _, _, _ := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: bare}})
		if len(turns) != 2 || turns[1].Marker == nil || turns[1].User != "" {
			t.Fatalf("turns = %+v", turns)
		}
		// Before the canonical input arrives the captured prompt is not shown either.
		bare.Entries = bare.Entries[:2]
		turns, _, _, _ = Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: bare}})
		if turns[1].User != "" {
			t.Fatalf("the captured prompt shows as %q", turns[1].User)
		}
	})
	t.Run("a queued continuation is an automatic row named by its marker", func(t *testing.T) {
		if len(queue) != 1 {
			t.Fatalf("queue = %+v", queue)
		}
		row := queue[0]
		// A package without a declared marker gets the generic label at render.
		if !row.Notice || row.Text != "" || row.Marker == nil || row.Marker.Label != "" {
			t.Fatalf("queued continuation = %+v, marker %+v", row, row.Marker)
		}
		// The strip draws it without an editor, so it gets no reader or
		// writer; a message queued beside it gets both.
		message := protocol.TurnID{5}
		both := page
		both.Queue.Items = append(slices.Clone(page.Queue.Items), protocol.ConversationPending{Turn: message, Kind: "task", Ready: true, Preview: "Next"})
		_, _, _, rows := Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: both}})
		cv := &viewmodel.Conversation{Queue: rows}
		(&Runtime{}).enrichQueue(cv, both.Queue)
		if len(cv.Queue) != 2 || cv.Queue[0].Load != nil || cv.Queue[0].Save != nil {
			t.Fatalf("the queued continuation can be read or saved: %+v", cv.Queue)
		}
		if cv.Queue[1].ID != message.String() || cv.Queue[1].Load == nil || cv.Queue[1].Save == nil {
			t.Fatalf("the queued message has no reader or writer: %+v", cv.Queue[1])
		}
	})
}
