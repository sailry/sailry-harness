package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// This fixture uses the pre-checkpoint store API so the full-read benchmark
// can be copied unchanged into the baseline checkout. Use a fixed iteration
// count so both checkouts refresh after the same sequence of committed appends.
type appendAssetRead func(context.Context, *sql.Conn, protocol.SessionID, protocol.ConversationAssetsQuery, string) (protocol.ConversationAssetsPage, string, bool, error)

func BenchmarkConversationAssetsAppendFull(b *testing.B) {
	benchmarkConversationAssetsAppend(b, false, func(ctx context.Context, db *sql.Conn, session protocol.SessionID, query protocol.ConversationAssetsQuery, _ string) (protocol.ConversationAssetsPage, string, bool, error) {
		page, err := ListConversationAssets(ctx, db, session, query)
		return page, "", false, err
	})
}

func benchmarkConversationAssetsAppend(b *testing.B, incremental bool, list appendAssetRead) {
	storage, _, session, turn, now := agentHistoryFixture(b, protocol.WorkModeCode)
	ctx := b.Context()
	var publication uint64
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "[Report](report.pdf)"))
		record := canonical.NewRecord(turn, "message", &message, now)
		record.Provenance.Author = "assistant"
		var err error
		publication, err = insertConversationAssetFixture(ctx, db, session.ID, record)
		if err != nil {
			return err
		}
		value := json.RawMessage(`{"stdout":"` + strings.Repeat("x", 128<<10) + `"}`)
		revision := fmt.Sprintf("%x", blake3.Sum256(value))
		for index := range 96 {
			response := &canonical.ToolResponse{ID: fmt.Sprint(index), Name: "fixture_output", Status: canonical.ToolSuccess, Revision: revision, Result: canonical.Payload{Value: value}}
			message := canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: response})
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "tool"
			if _, err := insertConversationAssetFixture(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		b.Fatal(err)
	}
	var checkpoint string
	refresh := func(wantDelta bool) int {
		b.Helper()
		query := protocol.ConversationAssetsQuery{Limit: 20}
		var groups []protocol.ConversationAssetsGroup
		for calls := 1; ; calls++ {
			var page protocol.ConversationAssetsPage
			var delta bool
			if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
				var err error
				page, checkpoint, delta, err = list(ctx, db, session.ID, query, checkpoint)
				return err
			}); err != nil {
				b.Fatal(err)
			}
			groups = append(groups, page.Groups...)
			if page.NextBefore == nil {
				if delta != wantDelta || delta && (len(groups) != 0 || calls != 1) || !delta && (len(groups) != 1 || groups[0].Sequence != publication) {
					b.Fatalf("refresh: calls=%d delta=%v groups=%d", calls, delta, len(groups))
				}
				return calls
			}
			if query.Before != nil && *page.NextBefore >= *query.Before {
				b.Fatal("asset cursor did not advance")
			}
			query.Before = page.NextBefore
			query.Limit = uint16(20 - len(groups))
		}
	}
	if refresh(false) < 3 {
		b.Fatal("long fixture did not span physical pages")
	}
	b.ReportAllocs()
	var calls int
	for b.Loop() {
		b.StopTimer()
		if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Continuing the existing report."))
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Provenance.Author = "assistant"
			_, err := insertConversationAssetFixture(ctx, db, session.ID, record)
			return err
		}); err != nil {
			b.Fatal(err)
		}
		b.StartTimer()
		calls += refresh(incremental)
	}
	b.ReportMetric(float64(calls)/float64(b.N), "pages/refresh")
}
