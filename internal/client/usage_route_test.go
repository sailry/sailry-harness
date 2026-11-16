package client

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUsageOverviewKeepsEqualTargetsOnDifferentEntries(t *testing.T) {
	direct := New(&clientTransport{})
	via := New(routedClientTransport{clientTransport: &clientTransport{}, entry: protocol.NodeID{9}})
	t.Cleanup(func() { _ = direct.Close(); _ = via.Close() })
	argument, err := json.Marshal(protocol.UsageQuery{Dimension: protocol.UsageDimensionProvider})
	if err != nil {
		t.Fatal(err)
	}
	observer, err := WatchOverview(t.Context(), []*Client{direct, via}, argument)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = observer.Close() })
	body, err := observer.Next(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	var value usageOverview
	if err := json.Unmarshal(body, &value); err != nil {
		t.Fatal(err)
	}
	if len(value.Sources) != 2 || value.Sources[0].Node != value.Sources[1].Node ||
		value.Sources[0].Entry == value.Sources[1].Entry {
		t.Fatalf("usage sources collapsed distinct entries: %+v", value.Sources)
	}
}

func TestUsageSummaryRetainsEntryForIdenticalPositions(t *testing.T) {
	target := protocol.NodeID{1}
	position := protocol.UsagePosition{Node: target, Sequence: 7, TimestampMs: 100}
	var sources []usageSource
	for _, entry := range []protocol.NodeID{{2}, {3}} {
		report := protocol.UsageReport{}
		report.Requests.Items = []protocol.UsageRequest{{Position: position}}
		sources = append(sources, usageSource{Node: target, Entry: entry, View: usageView{Report: &report}})
	}
	summary, err := summarizeUsage(sources)
	if err != nil || len(summary.Requests.Items) != 2 || summary.Requests.Items[0].Entry == summary.Requests.Items[1].Entry {
		t.Fatalf("same target request collapsed across entries: %+v %v", summary, err)
	}
}
