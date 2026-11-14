package model

import (
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestVisibleRecordsCanBeProjectedAgain(t *testing.T) {
	turn := protocol.NewTurnID()
	message := func(text string) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), time.Time{})
	}
	old, revealed, hidden := message("Old history"), message("Revealed input"), message("Hidden input")
	revealed.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Summary"))}, time.Time{})
	visibility := func(record canonical.Record, visible bool) canonical.Record {
		operation := canonical.NewRecord(turn, "operation", &canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: record.ID, AgentVisible: visible}}, time.Time{})
		operation.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
		return operation
	}
	records := []canonical.Record{old, summary, revealed, hidden, visibility(revealed, true), visibility(hidden, false)}
	once := VisibleRecords(records)
	if len(once) != 2 || once[0].ID != summary.ID || once[1].ID != revealed.ID {
		t.Fatalf("projection = %+v", once)
	}
	if twice := VisibleRecords(once); !reflect.DeepEqual(once, twice) {
		t.Fatal("reprojection lost the handoff or a visibility override")
	}
	if revealed.Metadata.Runtime.AgentVisible || !reflect.DeepEqual(once[1].Message(), revealed.Message()) {
		t.Fatal("projection mutated the original audience or replaced message content")
	}
}
