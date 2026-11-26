package output

import (
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCloneRecordMatchesJSONRoundTripAndIsolatesMutations(t *testing.T) {
	now := time.Date(2026, 11, 26, 2, 0, 0, 0, time.UTC)
	turnID := protocol.NewTurnID()
	providerID := protocol.NewProviderID()
	str := func(s string) *string { return &s }
	u64 := func(v uint64) *uint64 { return &v }
	i64 := func(v int64) *int64 { return &v }
	f64 := func(v float64) *float64 { return &v }
	bval := func(v bool) *bool { return &v }
	src := canonical.InputUser
	purpose := canonical.PurposeMain
	finish := canonical.Completed
	errCode := protocol.ErrorCodeInvalidRequest
	provMap := map[string]json.RawMessage{"k": json.RawMessage(`{"nested":1}`)}
	metaMap := map[string]json.RawMessage{"state": json.RawMessage(`"ok"`)}
	var nilTools []string

	msg := canonical.Message{
		Role:      canonical.Assistant,
		PartCount: 7,
		Model:     str("claude"),
		Finish:    &finish,
		Error:     &canonical.ResponseError{Code: str("rate_limit"), Message: "slow down", RetryExhausted: true},
		Usage: &canonical.Usage{
			Input: u64(100), Output: u64(50), Total: u64(150),
			CacheRead: u64(20), CacheWrite: u64(10), Reasoning: u64(5),
			Cost: f64(0.12), Provider: json.RawMessage(`null`),
			Pricing: &canonical.UsagePricing{Estimate: &protocol.UsageCost{UsdMicros: 120000, Responses: 1}},
		},
		Sources: canonical.Sources{
			Citations:         []canonical.Citation{{URI: str("https://example.test"), Title: str("Ex"), Start: i64(1), End: i64(5)}},
			Grounding:         []canonical.Grounding{{URI: "https://ground.test", Title: str("G")}},
			SearchSuggestions: str("try more"),
		},
		Content: []canonical.Block{
			{Index: 0, Kind: "text", Data: canonical.TextContent{Text: "hello"}},
			{Index: 1, Kind: "thinking", Data: &canonical.Thinking{Text: "hmm", Signature: str("sig")}},
			{Index: 2, Kind: "redacted_thinking", Data: &canonical.RedactedThinking{}},
			{Index: 3, Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Digest: "d1", Mime: "image/png", Size: 42, URI: str("u1"), Annotations: json.RawMessage(`{"audience":["assistant"]}`)}}},
			{Index: 4, Kind: "file", Data: &canonical.FileContent{File: canonical.FileRef{URI: "file:///a", Mime: str("text/plain")}}},
			{Index: 5, Kind: "tool_request", Data: &canonical.ToolRequest{
				ID: "call_1", Name: "read_file", Arguments: json.RawMessage(`{"path":"a.go"}`),
				Provider: &provMap, ToolMeta: json.RawMessage(`null`),
				Error: &canonical.ToolError{Code: -1, Message: "bad", Data: json.RawMessage(`{"x":1}`)},
			}},
			{Index: 6, Kind: "tool_response", Data: &canonical.ToolResponse{
				ID: "call_1", OriginalID: "orig_1", Name: "read_file", Status: canonical.ToolSuccess,
				Revision: "rev1", Format: canonical.MCP,
				Result: canonical.Payload{
					Value:  json.RawMessage(`{"content":[{"type":"text","text":"ok"}]}`),
					Assets: []canonical.BinarySlot{{Pointer: "/content/0", Encoding: canonical.Base64, Asset: canonical.AssetRef{Digest: "d2", Mime: "image/png", Size: 10}}},
				},
				Attachments: []canonical.Attachment{{Asset: canonical.AssetRef{Digest: "d3", Mime: "text/plain", Size: 3}}},
				Provider:    &provMap, Meta: &metaMap,
			}},
		},
	}

	records := []canonical.Record{
		{
			ID: "rec-msg", Created: now, Turn: turnID,
			Provenance: canonical.Provenance{Author: "assistant", Branch: "main"},
			Metadata: canonical.Metadata{
				Input: &canonical.Input{Turn: &turnID, Source: &src, TextPart: u64(0), NoticeIDs: []string{"n1"}},
				Presentation: &canonical.Presentation{
					Delegations: []string{"b", "a", "a"},
					Failures:    []canonical.ToolFailure{{Index: 1, ID: "c1", Name: "f", Failure: protocol.ToolFailure{Code: &errCode, Origin: "tool"}, Message: str("boom")}},
				},
				Execution: &canonical.Execution{
					Purpose:       &purpose,
					Source:        &canonical.ModelSource{ID: providerID, Name: "p", API: protocol.ConversationModelApiAnthropic, Authentication: protocol.AuthenticationApiKey, Model: "m"},
					Timing:        &canonical.Timing{ElapsedUs: u64(10), FirstTokenUs: u64(2)},
					Phases:        &protocol.ConversationObservationPhases{ActiveUs: u64(5)},
					ToolStarted:   &canonical.ToolStarted{ID: str("c1"), Name: "read_file"},
					ContextTokens: u64(1234),
				},
				Context: &canonical.Context{
					Captured: true, Title: str("t"), Guides: map[string]canonical.Guide{"g": {Path: "AGENTS.md", Revision: "r1", Content: "c"}},
					LoadedTools: &nilTools,
				},
				State:      &canonical.StateChanges{Session: map[string]json.RawMessage{"k": json.RawMessage(`1`)}},
				Compaction: &canonical.Maintenance{LatestInstruction: str("keep")},
				Runtime: &canonical.Runtime{
					Created: i64(100), After: str("prev"), UserVisible: true, AgentVisible: true,
					Inference: &canonical.Inference{Provider: "p", RequestedModel: "m", ResolvedModel: str("m2")},
					Notes: &canonical.OperationNotes{
						DeliveredSubagent:  str("sub-1"),
						CancelledSubagents: []string{"sub-2"},
						Context:            &canonical.PromptContext{State: "ok", Parts: map[string]string{"a": "b"}, Retained: []string{"r"}},
					},
					Logs: []string{"log1"},
				},
			},
			Kind: "message", Data: msg,
		},
		{
			ID: "rec-compact", Created: now, Turn: turnID, Kind: "compaction",
			Data: canonical.Compaction{
				Boundary: "rec-msg",
				Summary:  canonical.NewMessage(canonical.User, canonical.Text(0, "summary")),
				Start:    now, End: now,
				Files: []canonical.CompactedFile{{Path: "a.go", Modified: bval(true), Summary: str("edited")}},
				Native: &canonical.NativeCompaction{
					Source: canonical.ModelSource{ID: providerID, Name: "p", API: protocol.ConversationModelApiAnthropic, Authentication: protocol.AuthenticationApiKey, Model: "m"},
					Items:  json.RawMessage(`[1,2]`),
				},
			},
		},
		{
			ID: "rec-cfg", Created: now, Turn: turnID, Kind: "configuration",
			Data: canonical.Configuration{Context: canonical.Context{Captured: true, LoadedTools: &nilTools}},
		},
		{
			ID: "rec-op-vis", Created: now, Turn: turnID, Kind: "operation",
			Data: canonical.Operation{Kind: "visibility", Data: canonical.Visibility{Message: "rec-msg", UserVisible: true, AgentVisible: false}},
		},
		{
			ID: "rec-op-cancel", Created: now, Turn: turnID, Kind: "operation",
			Data: canonical.Operation{Kind: "note", Data: canonical.Note{Message: "rec-msg", Note: canonical.OperationNote{Kind: "subagents_cancelled", Data: canonical.SubagentsCancelled{}}}},
		},
	}

	for _, rec := range records {
		got := cloneRecord(rec, false)
		want := clone(rec)
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("record %s mismatch:\n got=%#v\nwant=%#v", rec.ID, got, want)
		}
	}

	// Verify borrowToolValues=true preserves OriginalID and isolates all mutable metadata/maps/slices.
	borrowed := cloneRecord(records[0], true)
	tr := response(&borrowed.MutableMessage().Content[6])
	if tr == nil || tr.OriginalID != "orig_1" {
		t.Fatalf("borrowToolValues lost OriginalID: %+v", tr)
	}
	(*tr.Meta)["state"][1] = 'X'
	(*tr.Provider)["k"][0] = 'X'
	tr.Result.Assets[0].Pointer = "/mutated"
	tr.Attachments[0].Asset.Digest = "mutated"
	borrowed.Metadata.Runtime.Logs[0] = "mutated"
	borrowed.Metadata.State.Session["k"][0] = '9'

	origTR := msg.Content[6].Data.(*canonical.ToolResponse)
	if string((*origTR.Meta)["state"]) != `"ok"` || string((*origTR.Provider)["k"]) != `{"nested":1}` ||
		origTR.Result.Assets[0].Pointer != "/content/0" || origTR.Attachments[0].Asset.Digest != "d3" ||
		records[0].Metadata.Runtime.Logs[0] != "log1" || string(records[0].Metadata.State.Session["k"]) != "1" {
		t.Fatal("mutating cloned record corrupted canonical source")
	}
}
