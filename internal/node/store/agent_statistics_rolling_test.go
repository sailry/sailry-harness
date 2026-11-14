package store

import (
	"database/sql"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAgentContextCountsRollingSummaryAndRetainedTail(t *testing.T) {
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		message := func(text string) canonical.Record {
			return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), now)
		}
		old, tail, growth := message("Compacted prefix"), message("Retained recent evidence"), message("New evidence after the summary")
		summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Rolling handoff")), Start: now, End: now}, now)
		purpose := canonical.PurposeCompaction
		summary.Metadata.Execution = &canonical.Execution{Purpose: &purpose, ContextTokens: ptr(uint64(30000))}
		for _, record := range []canonical.Record{old, tail, summary, growth} {
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		result, err := ReadAgentStatistics(ctx, db, session.ID)
		if err != nil {
			return err
		}
		want := estimateAgentRecords([]canonical.Record{summary, tail, growth})
		if result.ContextTokens != nil || result.ContextEstimate == nil || *result.ContextEstimate != want {
			return fmt.Errorf("rolling context estimate = %+v, want summary and retained tail tokens %d", result.ContextEstimate, want)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
