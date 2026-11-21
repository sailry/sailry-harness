package observation

import (
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestResponsesBoundaryCaptureLimits(t *testing.T) {
	items := `{"type":"message","role":"user","content":"bounded item"}`
	withItems := func(n int) []byte {
		return []byte(`{"input":[` + strings.Repeat(items+",", n-1) + items + `]}`)
	}
	for _, tc := range []struct {
		name     string
		admitted []byte
		skipped  []byte
		count    uint64
	}{
		{"items", withItems(maxResponsesItems), withItems(maxResponsesItems + 1), maxResponsesItems},
		{"bytes", []byte(`{"input":[]}` + strings.Repeat(" ", maxResponsesBody-len(`{"input":[]}`))), []byte(`{"input":[]}` + strings.Repeat(" ", maxResponsesBody+1-len(`{"input":[]}`))), 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var history ResponsesHistory
			_, trace := history.Start(t.Context(), protocol.NewSessionID(), protocol.NewTurnID())
			_, shape := trace.Prepare(tc.admitted, nil)
			if shape == nil || uint64(len(shape.input)) != tc.count || cap(shape.input) != int(tc.count) {
				t.Fatal("the exact limit must produce a bounded shape")
			}
			trace.Record("first", 1, nil, shape)
			boundary := trace.Record("second", 1, nil, shape)
			if got := boundary; got == nil || got.SharedInputItems != tc.count || got.PreviousInputItems != tc.count || got.InputItems != tc.count ||
				got.SharedInputBytes == nil || *got.SharedInputBytes != tc.count*uint64(len(items)) || len(got.InputChanges) != 0 || got.InputReorderIncomplete {
				t.Fatalf("comparison at the limit = %+v, want %d shared items", got, tc.count)
			}
			encoded, err := json.Marshal(boundary)
			if err != nil || len(encoded) > 400 || strings.Contains(string(encoded), "bounded item") {
				t.Fatalf("the serialized diagnostic must stay small and omit input: bytes=%d, err=%v", len(encoded), err)
			}
			_, skipped := trace.Prepare(tc.skipped, nil)
			if skipped != nil {
				t.Fatal("an over-limit body must not give a partial measurement")
			}
			trace.Record("skipped", 1, nil, skipped)
			if trace.Record("after-gap", 1, nil, shape) != nil {
				t.Fatal("comparison crossed an unmeasured request")
			}
		})
	}
}

func TestResponsesBoundarySkipsUnavailableInput(t *testing.T) {
	var history ResponsesHistory
	_, trace := history.Start(t.Context(), protocol.NewSessionID(), protocol.NewTurnID())
	for _, field := range []string{`"previous_response_id":"response"`, `"conversation":{"id":"conversation"}`, `"prompt":{"id":"template"}`} {
		_, shape := trace.Prepare([]byte(fmt.Sprintf(`{"input":[],%s}`, field)), nil)
		if shape != nil {
			t.Fatal("a reference to upstream input must leave the comparison unknown")
		}
	}
	for _, body := range []string{`{"input":[{"type":"item_reference","id":"item"}]}`, `{"input":[],"input":[{}]}`, `{"input":[}`} {
		_, shape := trace.Prepare([]byte(body), nil)
		if shape != nil {
			t.Fatal("an ambiguous or unresolved input must leave the comparison unknown")
		}
	}
}

func TestResponsesBoundaryCountsSerializedInputChanges(t *testing.T) {
	const (
		prefix       = `{"role":"user","content":"unchanged"}`
		user         = `{ "role":"user", "content":"user-秘密\n" }`
		assistant    = `{"type":"message","role":"assistant","content":"assistant-sensitive"}`
		newAssistant = `{"type":"message","role":"assistant","content":"assistant-added"}`
		toolCall     = `{"type":"function_call","name":"sensitive-tool-name","arguments":"sensitive-arguments"}`
		toolOutput   = `{"type":"function_call_output","output":"sensitive-result"}`
		developer    = `{"role":"developer","content":"sensitive-context"}`
		newDeveloper = `{"role":"system","content":"new-context"}`
		reasoning    = `{"type":"reasoning","encrypted_content":"sensitive-reasoning"}`
		unknown      = `{"type":"unrecognized-secret","role":"assistant"}`
		instructions = `"sensitive-\u0069nstructions"`
		updated      = `"updated\n上下文"`
	)
	size := func(items ...string) protocol.ConversationResponsesInputSize {
		var bytes uint64
		for _, item := range items {
			bytes += uint64(len(item))
		}
		return protocol.ConversationResponsesInputSize{Items: uint64(len(items)), Bytes: bytes}
	}
	compare := func(t *testing.T, prior, current string) *protocol.ConversationResponsesBoundary {
		t.Helper()
		var history ResponsesHistory
		_, trace := history.Start(t.Context(), protocol.NewSessionID(), protocol.NewTurnID())
		_, before := trace.Prepare([]byte(prior), nil)
		_, after := trace.Prepare([]byte(current), nil)
		trace.Record("before", 1, nil, before)
		got := trace.Record("after", 1, nil, after)
		if got == nil {
			t.Fatal("fixture bodies must be comparable")
		}
		return got
	}
	t.Run("mixed changes and local reorder", func(t *testing.T) {
		prior := `{"instructions":` + instructions + `,"input":[` + strings.Join([]string{prefix, developer, assistant, toolCall, toolOutput}, ", ") + `]}`
		current := `{"input":[` + strings.Join([]string{prefix, developer, toolCall, assistant, newAssistant, toolOutput, user, newDeveloper, reasoning, unknown}, ",\n") + `],"instructions":` + updated + `}`
		got := compare(t, prior, current)
		want := []protocol.ConversationResponsesInputChange{
			{Kind: protocol.ConversationResponsesInputUser, Added: size(user)},
			{Kind: protocol.ConversationResponsesInputAssistant, Added: size(newAssistant), Reordered: size(assistant)},
			{Kind: protocol.ConversationResponsesInputToolCall, Reordered: size(toolCall)},
			{Kind: protocol.ConversationResponsesInputDeveloperContext, Added: size(newDeveloper)},
			{Kind: protocol.ConversationResponsesInputReasoning, Added: size(reasoning)},
			{Kind: protocol.ConversationResponsesInputOther, Added: size(unknown)},
		}
		if got.SharedInputItems != 2 || got.PreviousInputItems != 5 || got.InputItems != 10 || got.SharedInputBytes == nil || *got.SharedInputBytes != size(prefix, developer).Bytes ||
			got.InstructionsEqual || got.InstructionsBytes == nil || *got.InstructionsBytes != (protocol.ConversationResponsesFieldBytes{Previous: uint64(len(instructions)), Current: uint64(len(updated))}) ||
			!got.OtherFieldsEqual || got.InputReorderIncomplete || !reflect.DeepEqual(got.InputChanges, want) {
			t.Fatalf("serialized comparison = %+v, want changes %+v", got, want)
		}
		encoded, err := json.Marshal(got)
		if err != nil || strings.Contains(string(encoded), "sensitive") || strings.Contains(string(encoded), "secret") || strings.Contains(string(encoded), "digest") {
			t.Fatalf("comparison leaked request contents or digests: bytes=%d, err=%v", len(encoded), err)
		}
	})
	t.Run("append tool results", func(t *testing.T) {
		got := compare(t, `{"input":[`+toolCall+`]}`, `{"input":[`+toolCall+`,`+toolOutput+`,`+toolOutput+`]}`)
		want := []protocol.ConversationResponsesInputChange{{Kind: protocol.ConversationResponsesInputToolOutput, Added: size(toolOutput, toolOutput)}}
		if got.InputReorderIncomplete || !reflect.DeepEqual(got.InputChanges, want) {
			t.Fatalf("appended results = %+v, want %+v", got.InputChanges, want)
		}
	})
	t.Run("ambiguous duplicate assignment", func(t *testing.T) {
		got := compare(t, `{"input":[`+assistant+`,`+user+`,`+assistant+`]}`, `{"input":[`+user+`,`+assistant+`]}`)
		want := []protocol.ConversationResponsesInputChange{{Kind: protocol.ConversationResponsesInputAssistant, Removed: size(assistant)}}
		if !got.InputReorderIncomplete || !reflect.DeepEqual(got.InputChanges, want) {
			t.Fatalf("duplicate removal = %+v, incomplete=%v, want %+v with incomplete order", got.InputChanges, got.InputReorderIncomplete, want)
		}
	})
	t.Run("scalar input preserves serialized escapes and quotes", func(t *testing.T) {
		const prior, current = `"\u0061"`, `"a"`
		got := compare(t, `{"input":`+prior+`}`, `{"input":`+current+`}`)
		want := []protocol.ConversationResponsesInputChange{{Kind: protocol.ConversationResponsesInputUser, Added: size(current), Removed: size(prior)}}
		if got.SharedInputBytes == nil || *got.SharedInputBytes != 0 || !reflect.DeepEqual(got.InputChanges, want) {
			t.Fatalf("serialized strings = %+v, want %+v", got.InputChanges, want)
		}
	})
}
