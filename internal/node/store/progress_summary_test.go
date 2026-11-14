package store

import (
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProgressSummaryRequiresSuccessfulDeclaredResult(t *testing.T) {
	arguments := json.RawMessage(`{"title":null,"steps":[{"description":"Inspect","state":"completed"}]}`)
	for _, format := range []canonical.ToolFormat{canonical.JSON, canonical.MCP} {
		t.Run(string(format), func(t *testing.T) {
			value := map[string]any{"progress": arguments}
			if format == canonical.MCP {
				value = map[string]any{"structuredContent": value}
			}
			body, err := json.Marshal(value)
			if err != nil {
				t.Fatal(err)
			}
			result := json.RawMessage(body)
			raw := &rawAgentTool{Name: "plugin_project_checklist", Arguments: arguments, Result: &result, Format: format, Presentation: protocol.ToolPresentationProgress, Grouping: protocol.ToolGroupingStandalone}
			summary, err := raw.summary()
			if err != nil {
				t.Fatal(err)
			}
			if summary.Progress == nil || len(summary.Progress.Steps) != 1 || summary.Progress.Steps[0].Description != "Inspect" {
				t.Fatalf("declared progress was not projected: %+v", summary)
			}
			raw.Presentation = protocol.ToolPresentationDetails
			if progress := raw.progress(); progress != nil {
				t.Fatalf("undeclared result became progress: %+v", progress)
			}
			raw.Presentation = protocol.ToolPresentationProgress
			// MCP can retain matching progress inside a failed structured result.
			if format == canonical.MCP {
				value["isError"] = true
				body, err = json.Marshal(value)
				if err != nil {
					t.Fatal(err)
				}
				failed := json.RawMessage(body)
				raw.Result = &failed
			}
			original := string(*raw.Result)
			for _, origin := range []protocol.ToolFailureOrigin{protocol.ToolFailureOriginTool, protocol.ToolFailureOriginRuntime} {
				raw.Failure = &protocol.ToolFailure{Origin: origin}
				summary, err = raw.summary()
				if err != nil {
					t.Fatal(err)
				}
				if summary.Progress != nil || summary.Failure == nil || !summary.Diagnostics || !summary.HasDetails {
					t.Fatalf("failed result hid its failure behind progress: %+v", summary)
				}
				if string(*raw.Result) != original {
					t.Fatal("progress projection changed the canonical result")
				}
			}
		})
	}
}
