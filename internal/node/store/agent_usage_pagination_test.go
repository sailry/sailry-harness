package store

import (
	"database/sql"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUsagePaginationAcrossNodes(t *testing.T) {
	t.Parallel()
	storage, _, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	// A report's Node is supplied by its serving Node, independently of stored records.
	node := protocol.NodeID{2}
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		for _, delta := range []time.Duration{-time.Millisecond, 0, 0, time.Millisecond} {
			message := canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Answer"))
			message.Usage = &canonical.Usage{Input: ptr(uint64(100)), Output: ptr(uint64(20))}
			record := canonical.NewRecord(turn, "message", &message, now.Add(delta))
			record.Provenance.Author = "assistant"
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	err = storage.Node.Read(ctx, func(db *sql.Conn) error {
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs, query.Dimension = now.Add(-time.Hour).UnixMilli(), now.Add(time.Hour).UnixMilli(), protocol.UsageDimensionModel
		baseline, err := readAgentUsage(ctx, db, node, query)
		if err != nil {
			return err
		}
		if len(baseline.Requests.Items) != 4 {
			t.Fatalf("unfiltered requests = %#v", baseline.Requests)
		}
		positions := make([]protocol.UsagePosition, 0, 4)
		for _, item := range baseline.Requests.Items {
			positions = append(positions, item.Position)
		}
		if positions[0].TimestampMs != now.Add(time.Millisecond).UnixMilli() || positions[1].TimestampMs != now.UnixMilli() || positions[2].TimestampMs != now.UnixMilli() || positions[1].Sequence <= positions[2].Sequence || positions[3].TimestampMs != now.Add(-time.Millisecond).UnixMilli() {
			t.Fatalf("single-Node request order = %#v", positions)
		}
		for _, test := range []struct {
			name   string
			before protocol.UsagePosition
			want   []protocol.UsagePosition
		}{
			{"lower Node includes equal timestamp regardless of sequence", protocol.UsagePosition{TimestampMs: now.UnixMilli(), Node: protocol.NodeID{3}}, positions[1:]},
			{"higher Node excludes equal timestamp regardless of sequence", protocol.UsagePosition{TimestampMs: now.UnixMilli(), Node: protocol.NodeID{1}, Sequence: ^uint64(0)}, positions[3:]},
			{"same Node keeps strict sequence boundary", positions[1], positions[2:]},
			{"same Node excludes the first equal timestamp", positions[2], positions[3:]},
			{"timestamp precedes Node ordering", protocol.UsagePosition{TimestampMs: now.Add(time.Millisecond).UnixMilli(), Node: protocol.NodeID{1}, Sequence: ^uint64(0)}, positions[1:]},
		} {
			t.Run(test.name, func(t *testing.T) {
				query.Before = &test.before
				report, err := readAgentUsage(ctx, db, node, query)
				if err != nil {
					t.Fatal(err)
				}
				got := make([]protocol.UsagePosition, 0, len(report.Requests.Items))
				for _, item := range report.Requests.Items {
					got = append(got, item.Position)
				}
				if !reflect.DeepEqual(got, test.want) {
					t.Fatalf("request positions = %#v, want %#v", got, test.want)
				}
				if !reflect.DeepEqual(report.Totals, baseline.Totals) || !reflect.DeepEqual(report.Groups, baseline.Groups) {
					t.Fatal("request pagination changed report aggregates")
				}
			})
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
