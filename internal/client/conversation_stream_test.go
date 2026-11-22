package client

import (
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func BenchmarkConversationTextStream(b *testing.B) {
	const token = "a fixed text delta for a deterministic replay of a plain reply. "
	for _, count := range []int{256, 1024, 4096} {
		b.Run(fmt.Sprint(count), func(b *testing.B) {
			s := conversationFixture()
			delta := protocol.ConversationDraft{ID: "reply", Turn: s.Page.Runs[0].Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: token}}}
			snapshot := protocol.Update{Kind: "conversation_snapshot", Data: s}
			frames := make([]protocol.Update, count)
			for i := range frames {
				frames[i] = conversationFrame(s, "delta", delta)
				s.Sequence++
			}
			want := strings.Repeat(token, count)
			b.ReportAllocs()
			for b.Loop() {
				p := NewConversationProjection(s.Node, s.Page.Session, 1)
				if got, err := p.Apply(1, snapshot); got != Applied || err != nil {
					b.Fatalf("snapshot: %v, %v", got, err)
				}
				for _, frame := range frames {
					if got, err := p.Apply(1, frame); got != Applied || err != nil {
						b.Fatalf("delta: %v, %v", got, err)
					}
				}
				if p.snapshot.Drafts[0].Parts[0].Data != want {
					b.Fatal("incorrect final text")
				}
			}
		})
	}
}

func TestConversationTextStreamPreservesPublishedPrefixes(t *testing.T) {
	s := conversationFixture()
	p := NewConversationProjection(s.Node, s.Page.Session, 1)
	apply := func(u protocol.Update) {
		t.Helper()
		if got, err := p.Apply(1, u); got != Applied || err != nil {
			t.Fatalf("apply %s: %v, %v", u.Kind, got, err)
		}
	}
	apply(protocol.Update{Kind: "conversation_snapshot", Data: s})
	delta := func(id, kind, text string) {
		t.Helper()
		apply(conversationFrame(*p.snapshot, "delta", protocol.ConversationDraft{ID: id, Turn: s.Page.Runs[0].Turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: kind, Data: text}}}))
	}
	delta("first", "text", "a")
	delta("first", "text", "b")
	published := p.Snapshot()
	delta("first", "text", "c")
	delta("other", "text", "another")
	delta("other", "text", " draft")
	delta("first", "text", "d")
	delta("first", "thinking", "new")
	delta("first", "thinking", " part")
	parts := p.snapshot.Drafts[0].Parts
	if published.Drafts[0].Parts[0].Data != "ab" || len(parts) != 2 || parts[0].Data != "abcd" || parts[1].Data != "new part" || p.snapshot.Drafts[1].Parts[0].Data != "another draft" {
		t.Fatal("appends must preserve published strings, part boundaries and interleaved drafts")
	}
	entry := protocol.ConversationEntry{ID: "first", Turn: s.Page.Runs[0].Turn, Sequence: 2, Author: "assistant", Parts: parts}
	apply(conversationFrame(*p.snapshot, "entry", entry))
	if p.draftText.Cap() != 0 || len(p.snapshot.Drafts) != 1 {
		t.Fatal("the committed draft kept its accumulation buffer")
	}
}
