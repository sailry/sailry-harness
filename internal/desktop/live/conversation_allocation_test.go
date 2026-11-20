package live

import (
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func fragmentedSnapshot(node protocol.NodeID, id protocol.SessionID, chunks, size int) protocol.ConversationSnapshot {
	s := protocol.DefaultConversationSnapshot()
	s.Node, s.Sequence, s.Page = node, 10, cacheTestPage(id, 1, id.String())
	s.Page.Entries = s.Page.Entries[:1]
	chunk := strings.Repeat("x", size)
	for i := range chunks {
		s.Page.Entries = append(s.Page.Entries, protocol.ConversationEntry{ID: fmt.Sprintf("reply/%d", i), Turn: s.Page.Runs[0].Turn, Sequence: uint64(i + 2), Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: chunk}}})
	}
	return s
}

// Measure total cold-projection allocations, independent of desktop thread
// scheduling, for a fixed reply size and for an ordinary multi-turn history.
func BenchmarkTranscriptProjectionAllocations(b *testing.B) {
	node, id := protocol.NodeID{1}, protocol.SessionID{2}
	measure := func(b *testing.B, s protocol.ConversationSnapshot, wantTurns, wantReply int) {
		b.Helper()
		state := client.ConversationState{Connected: true, Snapshot: s}
		var rows []conversation.Turn
		b.ReportAllocs()
		for b.Loop() {
			rows, _, _, _, _ = projectTranscript(state, "en")
		}
		if len(rows) != wantTurns || len(rows[0].Reply.Blocks) != 1 || len(rows[0].Reply.Blocks[0].Text) != wantReply {
			b.Fatal("fixture did not project its expected transcript")
		}
	}
	const replyBytes = 128 << 10
	for _, chunks := range []int{1, 128, 1024} {
		b.Run(fmt.Sprintf("128KiB/%d-entries", chunks), func(b *testing.B) {
			measure(b, fragmentedSnapshot(node, id, chunks, replyBytes/chunks), 1, replyBytes)
		})
	}
	b.Run("1000-ordered-turns", func(b *testing.B) {
		s := protocol.DefaultConversationSnapshot()
		s.Node = node
		s.Page = protocol.DefaultConversationPage()
		s.Page.Session = id
		const turns = 1000
		for i := 1; i <= turns; i++ {
			turn := protocol.TurnID{byte(i), byte(i >> 8)}
			start, end := int64(2*i), int64(2*i+1)
			s.Page.Runs = append(s.Page.Runs, protocol.ConversationRun{Session: id, Turn: turn, Sequence: uint64(i), Revision: 1, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusCompleted, StartedMs: &start, FinishedMs: &end})
			for j, author := range []string{"user", "assistant"} {
				s.Page.Entries = append(s.Page.Entries, protocol.ConversationEntry{ID: fmt.Sprintf("%d/%s", i, author), Turn: turn, Sequence: uint64(2*i + j - 1), Author: author, Parts: []protocol.ConversationPart{{Kind: "text", Data: "Message"}}})
			}
		}
		measure(b, s, turns, len("Message"))
	})
}
