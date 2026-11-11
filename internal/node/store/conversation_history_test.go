package store

import (
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Fitting sizes a page from entries encoded once. The sum must equal the real
// encoding to the byte, or a page one byte over the frame would be sent.
func TestConversationPageFit(t *testing.T) {
	t.Parallel()
	turns := []protocol.TurnID{protocol.NewTurnID(), protocol.NewTurnID(), protocol.NewTurnID()}
	build := func() protocol.ConversationPage {
		page := protocol.DefaultConversationPage()
		for index, turn := range turns {
			run := protocol.DefaultConversationRun()
			run.Turn, run.Kind, run.Status = turn, protocol.ConversationRunKindTask, protocol.ConversationStatusCompleted
			page.Runs = append(page.Runs, run)
			for part := range 2 {
				entry := protocol.DefaultConversationEntry()
				entry.ID, entry.Turn, entry.Sequence = fmt.Sprint("entry-", index, "-", part), turn, uint64(index*2+part+1)
				entry.Parts = []protocol.ConversationPart{{Kind: "text", Data: strings.Repeat("é<\n", 40*(index+1))}}
				page.Entries = append(page.Entries, entry)
			}
		}
		return page
	}
	size := func(page protocol.ConversationPage) int {
		body, err := encode(page)
		if err != nil {
			t.Fatal(err)
		}
		return len(body)
	}
	// headroom leaves exactly the given number of bytes for the page.
	headroom := func(budget int) int { return protocol.MAX_FRAME_BYTES - 64*1024 - budget }
	whole := build()
	exact := size(whole)

	t.Run("a page that fits to the byte is kept whole", func(t *testing.T) {
		page := build()
		trimmed, err := FitConversationPage(&page, headroom(exact))
		if err != nil || len(trimmed) != 0 || len(page.Entries) != len(whole.Entries) {
			t.Fatalf("fit = %d entries, trimmed %v, %v", len(page.Entries), trimmed, err)
		}
	})
	t.Run("one byte over drops only the oldest entry", func(t *testing.T) {
		page := build()
		trimmed, err := FitConversationPage(&page, headroom(exact-1))
		if err != nil || len(page.Entries) != len(whole.Entries)-1 || page.Entries[0].ID != whole.Entries[1].ID {
			t.Fatalf("fit = %d entries, %v", len(page.Entries), err)
		}
		if len(trimmed) != 1 || trimmed[0] != turns[0] {
			t.Fatalf("trimmed turns = %v", trimmed)
		}
	})
	t.Run("entries leave oldest first until the page fits", func(t *testing.T) {
		for budget := exact - 1; budget > exact/3; budget -= 97 {
			page := build()
			if _, err := FitConversationPage(&page, headroom(budget)); err != nil {
				t.Fatal(err)
			}
			kept := len(page.Entries)
			if size(page) > budget {
				t.Fatalf("budget %d: page of %d entries has %d bytes", budget, kept, size(page))
			}
			// The entry dropped last would not have fitted.
			if kept > 1 && kept < len(whole.Entries) {
				larger := build()
				larger.Entries = larger.Entries[len(larger.Entries)-kept-1:]
				if size(larger) <= budget {
					t.Fatalf("budget %d: dropped an entry that fitted", budget)
				}
			}
		}
	})
	t.Run("a closed turn leaves when its last entry cannot fit", func(t *testing.T) {
		page := build()
		last := protocol.DefaultConversationPage()
		last.Runs, last.Entries = page.Runs[1:], page.Entries[len(page.Entries)-1:]
		next := turns[1]
		last.NextBefore = &next
		budget := size(last)
		if _, err := FitConversationPage(&page, headroom(budget)); err != nil {
			t.Fatal(err)
		}
		if size(page) > budget || len(page.Runs) != 2 || page.Runs[0].Turn != turns[1] || page.NextBefore == nil || *page.NextBefore != turns[1] {
			t.Fatalf("pruned page = %d bytes of %d, %d runs, next %v", size(page), budget, len(page.Runs), page.NextBefore)
		}
		for _, entry := range page.Entries {
			if entry.Turn == turns[0] {
				t.Fatal("a pruned turn kept an entry")
			}
		}
	})
}
