package live

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDeclaredTableUsesCommonToolContent(t *testing.T) {
	table := protocol.ToolContentTable{Columns: []string{"Name"}, Rows: [][]string{{"example"}}, Truncated: true}
	result := json.RawMessage(`{"data":{"columns":["Name"],"rows":[["example"]]},"internal":"raw-result-marker"}`)
	var outputs []*conversation.Output
	for _, declared := range []bool{false, true} {
		loaded := protocol.ToolDetail{Result: &result}
		if declared {
			loaded.Resolved = &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{Table: &table}}
		} else {
			loaded.Content = &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{{Kind: "table", Data: table}}}
		}
		call := conversation.Call{Name: "fixture_query"}
		summary := protocol.ToolDetailSummary{HasDetails: true}
		ProjectToolDetail(&call, &summary, loaded, "en")
		if call.Output == nil || call.Output.Table == nil || !call.Output.Partial || call.Output.Text != "" {
			t.Fatalf("table declaration became raw output: %+v", call.Output)
		}
		outputs = append(outputs, call.Output)
	}
	if !reflect.DeepEqual(outputs[0], outputs[1]) {
		t.Fatalf("content and display tables use different projection: %+v, %+v", outputs[0], outputs[1])
	}
}

func TestDeclaredEmptyOutputDoesNotFallBackToJSON(t *testing.T) {
	result := json.RawMessage(`{"internal":"raw-result-marker"}`)
	loaded := protocol.ToolDetail{Result: &result, Resolved: &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{}}}
	for _, failed := range []bool{false, true} {
		summary := protocol.ToolDetailSummary{HasDetails: true}
		if failed {
			summary.Failure = &protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
		}
		call := conversation.Call{Name: "fixture_tool"}
		ProjectToolDetail(&call, &summary, loaded, "en")
		if call.Output == nil || strings.Contains(call.Output.Text, "raw-result-marker") != failed {
			t.Fatalf("unexpected raw fallback for failed=%t: %+v", failed, call.Output)
		}
	}
}

func TestToolNoticesKeepTheirLocalizedText(t *testing.T) {
	notice := protocol.PluginDesktopNavigation{Label: "Partial result", Locales: map[string]string{"fr": "Résultat partiel"}}
	result := json.RawMessage(`{"sailry_content":{},"text":""}`)
	for name, loaded := range map[string]protocol.ToolDetail{
		"declared": {Resolved: &protocol.ToolDetailResolved{Output: &protocol.ToolDetailOutput{
			Notices: []protocol.ToolResultLocalText{{Label: notice.Label, Locales: notice.Locales}},
		}}},
		"text": {Result: &result, Content: &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{
			{Kind: "text", Data: protocol.ToolContentText{Path: "/text", Notices: []protocol.PluginDesktopNavigation{notice}}},
		}}},
		"notice": {Content: &protocol.ToolContent{Version: 1, Blocks: []protocol.ToolContentBlock{
			{Kind: "notice", Data: protocol.ToolContentBlockNoticePayload{Message: notice}},
		}}},
	} {
		t.Run(name, func(t *testing.T) {
			for _, locale := range []string{"fr", "en"} {
				call := conversation.Call{Name: "fixture_tool"}
				summary := protocol.ToolDetailSummary{HasDetails: true}
				ProjectToolDetail(&call, &summary, loaded, locale)
				want := "Partial result"
				if locale == "fr" {
					want = "Résultat partiel"
				}
				if call.Output == nil || !reflect.DeepEqual(call.Output.Notices, []string{want}) || call.Output.Text != "" {
					t.Fatalf("%s notices changed: %+v", locale, call.Output)
				}
			}
		})
	}
}
