package engine

import (
	"encoding/json"
	"fmt"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReadSessionOverviewAndUsageBuiltins(t *testing.T) {
	m := &scriptedModel{}
	f := newFixtureWithTimeout(t, m, 15*time.Second)
	charged := finish()
	charged.ProviderMetadata = fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{
		Reported: true, Input: new(int64(40)), Output: new(int64(12)), CacheRead: new(int64(6)), CacheWrite: new(int64(3)),
	}}
	m.scripts = []script{response(text("The original plan is ready."), charged)}
	f.start()
	past := f.submit("Keep the original migration plan.")
	f.waitRun(past, protocol.ConversationStatusCompleted)

	m.mu.Lock()
	m.scripts = append(m.scripts,
		response(call("load-history", "tool_search", `{"query":"select:read_session"}`), finish()),
		response(
			call("overview", callToolName, fmt.Sprintf(`{"name":"read_session","arguments":{"mode":"overview","session":%q}}`, f.session.ID.String())),
			call("usage", callToolName, fmt.Sprintf(`{"name":"read_session","arguments":{"mode":"usage","session":%q,"turn":%q}}`, f.session.ID.String(), past.String())),
			finish(),
		),
		response(text("The history and request usage are available."), finish()),
	)
	m.mu.Unlock()
	lookup := f.submit("Find the plan and the token usage for that reply.")
	f.waitRun(lookup, protocol.ConversationStatusCompleted)
	results := map[string]*canonical.ToolResponse{}
	for _, record := range f.state().Records {
		if message := record.Message(); record.Turn == lookup && message != nil {
			for _, block := range message.Content {
				if result, ok := block.Data.(*canonical.ToolResponse); ok {
					results[result.ID] = result
				}
			}
		}
	}
	check := func(id string, target any) {
		t.Helper()
		result := results[id]
		if result == nil || result.Name != "read_session" || result.Status != canonical.ToolSuccess {
			t.Fatalf("%s did not dispatch read_session successfully: %+v", id, result)
		}
		if err := json.Unmarshal(result.Result.Value, target); err != nil {
			t.Fatal(err)
		}
	}
	var overview struct {
		InitialIntent struct {
			Turn protocol.TurnID `json:"turn"`
			Text string          `json:"text"`
		} `json:"initial_intent"`
		Turns []struct {
			Turn  protocol.TurnID `json:"turn"`
			Reply struct {
				EntrySequence uint64 `json:"entry_sequence"`
				Text          string `json:"text"`
			} `json:"reply"`
		} `json:"turns"`
	}
	check("overview", &overview)
	if overview.InitialIntent.Turn != past || overview.InitialIntent.Text != "Keep the original migration plan." || len(overview.Turns) != 2 || overview.Turns[1].Turn != past || overview.Turns[1].Reply.Text != "The original plan is ready." {
		t.Fatalf("wrong overview: %+v", overview)
	}
	type tokenCounts struct {
		Input       uint64 `json:"input"`
		Output      uint64 `json:"output"`
		CachedInput uint64 `json:"cached_input"`
		CacheWrite  uint64 `json:"cache_write"`
	}
	var usage struct {
		Turn              protocol.TurnID `json:"turn"`
		ReportedResponses uint64          `json:"reported_responses"`
		Usage             tokenCounts     `json:"usage"`
		Requests          []struct {
			EntrySequence uint64      `json:"entry_sequence"`
			Usage         tokenCounts `json:"usage"`
		} `json:"requests"`
	}
	check("usage", &usage)
	want := tokenCounts{Input: 40, Output: 12, CachedInput: 6, CacheWrite: 3}
	if usage.Turn != past || usage.ReportedResponses != 1 || usage.Usage != want || len(usage.Requests) != 1 || usage.Requests[0].Usage != want || usage.Requests[0].EntrySequence == 0 || usage.Requests[0].EntrySequence != overview.Turns[1].Reply.EntrySequence {
		t.Fatalf("wrong usage result: %+v", usage)
	}
}
