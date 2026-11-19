package store

import (
	"database/sql"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// BenchmarkUsageReport measures one report over 20,000 priced responses.
func BenchmarkUsageReport(b *testing.B) {
	storage, node, session, turn, now := agentHistoryFixture(b, protocol.WorkModeCode)
	ctx := b.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		price := `{"cost":{"input":3,"output":15,"cache_read":0.3,"cache_write":3.75,"tiers":[{"tier":{"type":"context","size":150},"input":6,"output":30,"cache_read":0.6,"cache_write":7.5}]}}`
		if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)", price); err != nil {
			return err
		}
		for i := range 20000 {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, fmt.Sprint("Answer ", i)))
			message.Usage = &canonical.Usage{Input: ptr(uint64(100 + i%200)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(10)), Reasoning: ptr(uint64(5)), Provider: []byte(`{"prompt_tokens_details":{"cached_tokens":10}}`)}
			record := canonical.NewRecord(turn, "message", &message, now.Add(-time.Duration(i)*time.Second))
			record.Provenance.Author = "assistant"
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		b.Fatal(err)
	}
	query := protocol.DefaultUsageQuery()
	query.StartMs, query.EndMs, query.Dimension = now.Add(-24*time.Hour).UnixMilli(), now.Add(time.Hour).UnixMilli(), protocol.UsageDimensionModel
	b.ReportAllocs()
	for b.Loop() {
		err = storage.Node.Read(ctx, func(db *sql.Conn) error {
			report, err := readAgentUsage(ctx, db, node, query)
			if err == nil && (report.Totals.Responses != 20000 || report.Totals.Cost == nil || report.Totals.Cost.Responses != 20000) {
				err = fmt.Errorf("report totals = %#v", report.Totals)
			}
			return err
		})
		if err != nil {
			b.Fatal(err)
		}
	}
}
