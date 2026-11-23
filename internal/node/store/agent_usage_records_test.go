package store

import (
	"database/sql"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUsageRecordsBackfillOnUpgradeAndSkipLargeEventBlobs(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	profile := storage.Profile
	padding := strings.Repeat("large-assistant-output-", 1024)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)", []byte(`{"cost":{"input":2,"output":8,"cache_read":0.5,"cache_write":2.5}}`)); err != nil {
			return err
		}
		for i := range 5 {
			msg := canonical.NewMessage(canonical.Assistant, canonical.Text(0, padding))
			msg.Usage = &canonical.Usage{
				Input:      ptr(uint64(100 + i)),
				Output:     ptr(uint64(20)),
				CacheRead:  ptr(uint64(10)),
				CacheWrite: ptr(uint64(5)),
				Provider:   []byte(`{"cache_creation":{"ephemeral_5m_input_tokens":5,"ephemeral_1h_input_tokens":0},"extra":"drop-me"}`),
			}
			record := canonical.NewRecord(turn, "message", &msg, now.Add(time.Duration(i)*time.Second))
			record.Provenance.Author = "assistant"
			record.Metadata.Execution = &canonical.Execution{Timing: &canonical.Timing{ElapsedUs: ptr(uint64(500)), FirstTokenUs: ptr(uint64(100))}}
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		// Append an unmetered message that must not enter agent_usage_records.
		plain := canonical.NewMessage(canonical.Assistant, canonical.Text(0, padding))
		if _, _, err := AppendCanonical(ctx, db, session.ID, canonical.NewRecord(turn, "message", &plain, now.Add(10*time.Second))); err != nil {
			return err
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	query := protocol.DefaultUsageQuery()
	query.StartMs, query.EndMs, query.Dimension = now.Add(-time.Hour).UnixMilli(), now.Add(time.Hour).UnixMilli(), protocol.UsageDimensionModel
	var before protocol.UsageReport
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		before, err = readAgentUsage(ctx, db, node, query)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if before.Totals.Responses != 5 || before.Totals.Tokens == nil || before.Totals.Tokens.Input != 510 {
		t.Fatalf("unexpected initial usage totals: %+v", before.Totals)
	}

	// Simulate an older database that lacks agent_usage_records and reopen to trigger upgrade + backfill.
	schemaExecute(t, storage.Node, "DROP TABLE agent_usage_records")
	if err := storage.Close(); err != nil {
		t.Fatal(err)
	}
	reopened := reopenSchemaFixture(t, profile, node)
	if count := schemaInteger(t, reopened.Node, "SELECT count(*) FROM agent_usage_records"); count != 5 {
		t.Fatalf("backfilled agent_usage_records rows = %d, want 5", count)
	}
	var after protocol.UsageReport
	if err := reopened.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		after, err = readAgentUsage(ctx, db, node, query)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if after.Totals.Responses != before.Totals.Responses || after.Totals.Tokens == nil || *after.Totals.Tokens != *before.Totals.Tokens || after.Totals.Cost == nil || before.Totals.Cost == nil || after.Totals.Cost.UsdMicros != before.Totals.Cost.UsdMicros {
		t.Fatalf("backfilled report diverged: got %+v, want %+v", after.Totals, before.Totals)
	}
}
