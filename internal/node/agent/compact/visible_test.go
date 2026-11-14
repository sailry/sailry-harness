package compact

import (
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRollingSummaryPrecedesRetainedTail(t *testing.T) {
	turn := protocol.NewTurnID()
	message := func(text string) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), time.Time{})
	}
	old, recent, revealed, hidden, growth := message("Old history"), message("Recent request"), message("Retained evidence"), message("Hidden evidence"), message("New request")
	revealed.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
	hidden.Metadata.Runtime = &canonical.Runtime{AgentVisible: false}
	empty := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant), time.Time{})
	visibility := canonical.NewRecord(turn, "operation", &canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: revealed.ID, AgentVisible: true}}, time.Time{})
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Rolling handoff"))}, time.Time{})
	records := []canonical.Record{old, recent, revealed, hidden, empty, visibility, summary, growth}
	before, err := json.Marshal(records)
	if err != nil {
		t.Fatal(err)
	}
	for _, row := range []struct {
		name    string
		project func([]canonical.Record) []canonical.Record
		ids     []string
	}{
		{"transcript", Visible, []string{summary.ID, recent.ID, revealed.ID, growth.ID}},
		{"pending", Pending, []string{summary.ID, recent.ID, revealed.ID, empty.ID, growth.ID}},
	} {
		t.Run(row.name, func(t *testing.T) {
			projected := row.project(records)
			var ids []string
			for _, record := range projected {
				ids = append(ids, record.ID)
			}
			if !reflect.DeepEqual(ids, row.ids) {
				t.Fatalf("summary and retained tail = %v, want %v", ids, row.ids)
			}
			if !reflect.DeepEqual(row.project(projected), projected) {
				t.Fatal("reprojection changed summary order or restored visibility")
			}
		})
	}
	next := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: recent.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Updated handoff"))}, time.Time{})
	projected := Visible(append(append([]canonical.Record(nil), records...), next))
	if len(projected) != 3 || projected[0].ID != next.ID || projected[1].ID != revealed.ID || projected[2].ID != growth.ID {
		t.Fatal("latest summary did not replace its prefix and the previous summary")
	}
	after, err := json.Marshal(records)
	if err != nil || string(before) != string(after) {
		t.Fatal("projection changed canonical history")
	}
}
