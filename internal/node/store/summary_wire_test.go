package store

import (
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func TestToolSummaryOmitsUndisclosedOutput(t *testing.T) {
	const source = "package source_body_marker\nfunc example() {}\n"
	body, err := json.Marshal(map[string]any{"text": source, "path": "result-only-path.go"})
	if err != nil {
		t.Fatal(err)
	}
	original := string(body)
	result := json.RawMessage(body)
	text := protocol.ToolDisplayProjectionReference{Source: protocol.ToolDisplayProjectionSourceResult, Path: "/text"}
	path := protocol.ToolDisplayProjectionReference{Source: protocol.ToolDisplayProjectionSourceResult, Path: "/path"}
	display := &protocol.ToolDisplay{
		Label: "Read file",
		Input: &protocol.ToolDisplayInput{Summary: &protocol.ToolDisplayField{Path: "/path", Code: true}},
		Output: &protocol.ToolDisplayProjectionOutput{
			Preview: &protocol.ToolDisplayProjectionText{Parts: []protocol.ToolDisplayProjectionPart{{Value: &text}}},
			Body:    &protocol.ToolDisplayProjectionBody{Text: text, Path: &path},
			Paths:   &protocol.ToolDisplayProjectionPaths{Value: path},
		},
	}
	for _, mode := range []protocol.ToolPresentation{
		protocol.ToolPresentationSummary,
		protocol.ToolPresentationHidden,
		protocol.ToolPresentationDetails,
		protocol.ToolPresentationContent,
	} {
		t.Run(string(mode), func(t *testing.T) {
			raw := &rawAgentTool{Name: "read_file", Arguments: json.RawMessage(`{"path":"input.go"}`), Result: &result,
				Display: display, Presentation: mode, Grouping: protocol.ToolGroupingSequence}
			part, err := raw.part()
			if err != nil {
				t.Fatal(err)
			}
			summary := part.Data.(*protocol.ConversationPartToolResultPayload).Summary
			if summary.Resolved == nil || summary.Resolved.Label == nil || summary.Resolved.Label.Label != "Read file" ||
				summary.Resolved.Input.Summary == nil || summary.Resolved.Input.Summary.Text != "input.go" || !summary.Resolved.Input.Summary.Code {
				t.Fatalf("input identity lost in summary: %+v", summary.Resolved)
			}
			expandable := mode == protocol.ToolPresentationDetails || mode == protocol.ToolPresentationContent
			if summary.HasDetails != expandable || (summary.Resolved.Output != nil) != expandable {
				t.Fatalf("unexpected disclosure for %s: %+v", mode, summary)
			}
			wire, err := json.Marshal(part)
			if err != nil {
				t.Fatal(err)
			}
			for _, marker := range []string{"source_body_marker", "result-only-path.go"} {
				if strings.Contains(string(wire), marker) != expandable {
					t.Fatalf("unexpected result content in %s projection: %s", mode, wire)
				}
			}
			full := raw.resolved(true, raw.displayData())
			if full == nil || full.Output == nil || full.Output.Preview != source || full.Output.Body == nil || full.Output.Body.Text != source {
				t.Fatalf("explicit detail lost original output: %+v", full)
			}
			if string(*raw.Result) != original {
				t.Fatal("summary changed the canonical result")
			}
		})
	}
}

func TestSummaryFailuresKeepDetails(t *testing.T) {
	const message = "Permission denied"
	result := json.RawMessage(`{"error":"Permission denied"}`)
	errorValue := protocol.ToolDisplayProjectionReference{Source: protocol.ToolDisplayProjectionSourceResult, Path: "/error"}
	for _, mode := range []protocol.ToolPresentation{protocol.ToolPresentationSummary, protocol.ToolPresentationHidden} {
		t.Run(string(mode), func(t *testing.T) {
			raw := &rawAgentTool{Name: "read_file", Arguments: json.RawMessage(`{"path":"input.go"}`), Result: &result,
				Presentation: mode, Grouping: protocol.ToolGroupingSequence,
				Failure: &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime},
				Display: &protocol.ToolDisplay{Label: "Read file", Output: &protocol.ToolDisplayProjectionOutput{
					Preview: &protocol.ToolDisplayProjectionText{Parts: []protocol.ToolDisplayProjectionPart{{Value: &errorValue}}},
				}},
			}
			summary, err := raw.summary()
			if err != nil {
				t.Fatal(err)
			}
			if !summary.HasDetails || !summary.Diagnostics || summary.Failure == nil || summary.Error == nil || *summary.Error != message {
				t.Fatalf("short failure lost its details: %+v", summary)
			}
			if summary.Resolved == nil || summary.Resolved.Output != nil {
				t.Fatalf("failure eagerly projected output: %+v", summary.Resolved)
			}
			full := raw.resolved(true, raw.displayData())
			if full == nil || full.Output == nil || full.Output.Preview != message {
				t.Fatalf("original failure was lost from requested details: %+v", full)
			}
		})
	}
}

func TestUndeclaredSummaryDoesNotAcquireOutput(t *testing.T) {
	result := json.RawMessage(`{"text":"canonical-only-source"}`)
	raw := &rawAgentTool{Name: "read_file", Arguments: json.RawMessage(`{"path":"input.go"}`), Result: &result,
		Presentation: protocol.ToolPresentationSummary, Grouping: protocol.ToolGroupingSequence}
	part, err := raw.part()
	if err != nil {
		t.Fatal(err)
	}
	summary := part.Data.(*protocol.ConversationPartToolResultPayload).Summary
	if summary.HasDetails || summary.Resolved != nil {
		t.Fatalf("undeclared summary acquired an output: %+v", summary)
	}
	wire, err := json.Marshal(part)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(wire), "canonical-only-source") {
		t.Fatalf("undeclared output leaked into the conversation projection: %s", wire)
	}
}

func TestFailureOverridesSuccessPreview(t *testing.T) {
	const message = "bound conversations cannot create another connection"
	const original = `{"error":{"code":"permission_denied","message":"bound conversations cannot create another connection"}}`
	result := json.RawMessage(original)
	punctuation := "@:"
	for _, mode := range []protocol.ToolPresentation{protocol.ToolPresentationDetails, protocol.ToolPresentationContent} {
		t.Run(string(mode), func(t *testing.T) {
			raw := &rawAgentTool{Name: "ssh_save_connection", Arguments: json.RawMessage(`{"name":"Another host"}`), Result: &result,
				Presentation: mode, Grouping: protocol.ToolGroupingSequence, Failure: &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime},
				Display: &protocol.ToolDisplay{Label: "Save SSH connection", Input: &protocol.ToolDisplayInput{Summary: &protocol.ToolDisplayField{Path: "/name"}},
					Output: &protocol.ToolDisplayProjectionOutput{Preview: &protocol.ToolDisplayProjectionText{Parts: []protocol.ToolDisplayProjectionPart{{Text: &punctuation}}}}},
			}
			for _, full := range []bool{false, true} {
				resolved := raw.resolved(full, raw.displayData())
				if resolved.Output == nil || resolved.Output.Preview != message || resolved.Output.Body != nil || resolved.Input.Summary == nil || resolved.Input.Summary.Text != "Another host" {
					t.Fatalf("failure was replaced by a success recipe: %+v", resolved)
				}
			}
			if string(*raw.Result) != original {
				t.Fatal("failure projection changed the original result")
			}
		})
	}
}

func TestToolSummaryPreservesRevisionAndNormalizedSize(t *testing.T) {
	result := json.RawMessage(`{ "z": [9007199254740999, "<b>"], "a": 1, "a": 2 }`)
	for _, output := range []*json.RawMessage{nil, &result} {
		raw := rawAgentTool{Name: "test", Arguments: json.RawMessage(`{"path":"a&b"}`), Result: output, Presentation: protocol.ToolPresentationSummary}
		args := raw.displayData().args
		original, err := encode([]any{raw.ID, raw.Name, args, raw.result(), raw.Display, raw.Presentation, raw.Grouping, raw.Images, raw.Failure, raw.Error})
		if err != nil {
			t.Fatal(err)
		}
		sum := blake3.Sum256(original)
		summary, err := raw.summary()
		if err != nil {
			t.Fatal(err)
		}
		if summary.Revision != hex.EncodeToString(sum[:]) {
			t.Fatal("tool detail revision changed")
		}
		if output == nil {
			if summary.Bytes != nil {
				t.Fatal("request has a result size")
			}
		} else {
			encoded, err := json.Marshal(raw.result())
			if err != nil {
				t.Fatal(err)
			}
			if summary.Bytes == nil || *summary.Bytes != uint64(len(encoded)) {
				t.Fatal("normalized result size changed")
			}
		}
	}
}
