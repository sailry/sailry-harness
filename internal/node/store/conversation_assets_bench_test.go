package store

import (
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func BenchmarkListConversationAssets(b *testing.B) {
	for _, name := range []string{"inline_output", "externalized_output"} {
		b.Run(name, func(b *testing.B) {
			storage, _, session, turn, now := agentHistoryFixture(b, protocol.WorkModeCode)
			ctx := b.Context()
			const count, size = 8, 448 << 10
			var publication uint64
			if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
				text := strings.Repeat("abcd", size/4)
				value := json.RawMessage(`{"kind":"command_result","data":{"stdout":{"text":"` + text + `"}}}`)
				payload := canonical.Payload{Value: value}
				if name == "externalized_output" {
					value = json.RawMessage(`{"data":"` + base64.StdEncoding.EncodeToString([]byte(text)) + `","mime_type":"application/octet-stream"}`)
					var err error
					payload, err = CaptureCanonicalPayload(ctx, db, value)
					if err != nil {
						return err
					}
					if len(payload.Assets) != 1 {
						return fmt.Errorf("benchmark did not externalize its tool output")
					}
				}
				record := canonical.NewRecord(turn, "message", ptr(canonical.NewMessage(canonical.Assistant, canonical.Text(0, "[Report](report.pdf)"))), now)
				record.Provenance.Author = "assistant"
				var err error
				publication, err = insertConversationAssetFixture(ctx, db, session.ID, record)
				if err != nil {
					return err
				}
				revision := fmt.Sprintf("%x", blake3.Sum256(value))
				for index := range count {
					response := &canonical.ToolResponse{ID: fmt.Sprint(index), Name: "fixture_output", Status: canonical.ToolSuccess, Revision: revision, Result: payload}
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
			b.ReportAllocs()
			var page protocol.ConversationAssetsPage
			for b.Loop() {
				if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
					var err error
					page, err = ListConversationAssets(ctx, db, session.ID, protocol.ConversationAssetsQuery{Limit: 20})
					return err
				}); err != nil {
					b.Fatal(err)
				}
			}
			if page.NextBefore != nil || len(page.Groups) != 1 || page.Groups[0].Sequence != publication || len(page.Groups[0].Items) != 1 {
				b.Fatalf("unexpected inventory: %+v", page)
			}
			if file, ok := page.Groups[0].Items[0].Data.(*protocol.ConversationAssetsTargetFilePayload); !ok || file.Path != "report.pdf" {
				b.Fatalf("benchmark lost its published file: %+v", page.Groups[0].Items)
			}
		})
	}
}

func insertConversationAssetFixture(ctx context.Context, db *sql.Conn, session protocol.SessionID, record canonical.Record) (uint64, error) {
	body, err := encode(record)
	if err != nil {
		return 0, err
	}
	sequence, err := NextCounter(ctx, db, CounterAgentEvents)
	if err != nil {
		return 0, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_events(sequence,session,id,turn,body) VALUES(?1,?2,?3,?4,?5)", sequence, session.String(), record.ID, record.Turn.String(), body); err != nil {
		return 0, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO conversation_events(session,sequence) VALUES(?1,?2)", session.String(), sequence)
	return sequence, err
}
