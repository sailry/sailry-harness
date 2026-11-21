package store

import (
	"context"
	"database/sql"
	"fmt"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
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
		original := page.Entries
		trimmed, err := FitConversationPage(&page, headroom(exact-1))
		if err != nil || len(page.Entries) != len(whole.Entries)-1 || page.Entries[0].ID != whole.Entries[1].ID {
			t.Fatalf("fit = %d entries, %v", len(page.Entries), err)
		}
		if original[0].ID != "" || original[0].Parts != nil {
			t.Fatal("discarded entry retained its payload in the fitted slice backing array")
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
	t.Run("pruning takes the recipient and interleaved guidance together", func(t *testing.T) {
		page := build()
		page.Runs[2].Joined = &turns[0]
		kept := protocol.DefaultConversationPage()
		kept.Runs = page.Runs[1:2]
		kept.NextBefore = &turns[1]
		if _, err := FitConversationPage(&page, headroom(size(kept))); err != nil {
			t.Fatal(err)
		}
		if len(page.Runs) != 1 || page.Runs[0].Turn != turns[1] || len(page.Entries) != 0 ||
			page.NextBefore == nil || *page.NextBefore != turns[1] {
			t.Fatalf("a pruned guidance group left data or skipped the next group: %+v", page)
		}
	})
	t.Run("snapshot fit reuses lengths across both trims", func(t *testing.T) {
		page, refitted := build(), build()
		var fit conversationPageFit
		for _, budget := range []int{exact - 1, exact / 2, exact / 3} {
			got, err := fit.apply(&page, headroom(budget), true)
			want, refitErr := FitConversationPage(&refitted, headroom(budget))
			gotJSON, _ := encode(page)
			wantJSON, _ := encode(refitted)
			if err != nil || refitErr != nil || !slices.Equal(got, want) || !slices.Equal(gotJSON, wantJSON) || len(gotJSON) > budget {
				t.Fatalf("budget %d: reusing lengths changed the page or missing turns (%v, %v)", budget, err, refitErr)
			}
		}
	})
}

func BenchmarkConversationSnapshotFitting(b *testing.B) {
	page := protocol.DefaultConversationPage()
	turn := protocol.NewTurnID()
	text := strings.Repeat("a", 32<<10)
	for i := range 96 {
		page.Entries = append(page.Entries, protocol.ConversationEntry{
			ID: fmt.Sprint(i), Turn: turn, Sequence: uint64(i + 1), Author: "assistant",
			Parts: []protocol.ConversationPart{{Kind: "text", Data: text}},
		})
	}
	for _, test := range []struct {
		name  string
		reuse bool
	}{{"measure_twice", false}, {"reuse_entry_lengths", true}} {
		b.Run(test.name, func(b *testing.B) {
			b.ReportAllocs()
			for b.Loop() {
				value, fit := page, conversationPageFit{}
				if _, err := fit.apply(&value, 0, true); err != nil {
					b.Fatal(err)
				}
				if !test.reuse {
					fit = conversationPageFit{}
				}
				if _, err := fit.apply(&value, 1<<20, true); err != nil || len(value.Entries) != len(page.Entries) {
					b.Fatalf("fixture did not fit: %v", err)
				}
			}
		})
	}
}

func TestConversationMissingHonorsForkVisibility(t *testing.T) {
	t.Parallel()
	storage, _, source, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := context.Background()
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		first := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "First input")), now)
		first.Provenance.Author = "user"
		if _, _, err := AppendCanonical(ctx, db, source.ID, first); err != nil {
			return err
		}
		for range 100 {
			message := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Recent history")), now)
			message.Provenance.Author = "assistant"
			if _, _, err := AppendCanonical(ctx, db, source.ID, message); err != nil {
				return err
			}
		}
		setVisibility := func(visible bool) error {
			record := canonical.NewRecord(turn, "operation", canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{
				Message: first.ID, UserVisible: visible, AgentVisible: visible,
			}}, now)
			runtime := canonical.DefaultRuntime()
			runtime.UserVisible = false
			record.Metadata.Runtime = &runtime
			_, _, err := AppendCanonical(ctx, db, source.ID, record)
			return err
		}
		if err := setVisibility(false); err != nil {
			return err
		}
		if _, _, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusCompleted, nil, now); err != nil {
			return err
		}
		fork, _, err := forkConversation(ctx, db, protocol.CommandForkConversationPayload{Session: source.ID, Through: turn, ExpectedRevision: source.Revision})
		if err != nil {
			return err
		}
		// The new visibility operation belongs only to the source session.
		if err = setVisibility(true); err != nil {
			return err
		}
		for _, row := range []struct {
			session protocol.SessionID
			missing []protocol.TurnID
		}{{source.ID, []protocol.TurnID{turn}}, {fork.ID, nil}} {
			history, err := ReadConversation(ctx, db, row.session, nil, 20)
			if err != nil {
				return err
			}
			if len(history.Page.Entries) != 100 || !slices.Equal(history.Missing, row.missing) {
				return fmt.Errorf("session %s has %d entries and missing %v, want 100 and %v", row.session, len(history.Page.Entries), history.Missing, row.missing)
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}
