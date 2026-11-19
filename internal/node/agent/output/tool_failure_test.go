package output

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

func failedRecord(body string, format canonical.ToolFormat, attachments ...canonical.Attachment) canonical.Record {
	record := resultRecord("failed", json.RawMessage(body), format)
	response := onlyResponse(&record)
	response.Status = canonical.ToolErrorStatus
	response.Attachments = attachments
	return record
}

func TestDiagnosticOnlyFailuresCollapseToReadableText(t *testing.T) {
	for _, row := range []struct {
		name, body, want string
		format           canonical.ToolFormat
	}{
		{"mcp text", `{"isError":true,"content":[{"type":"text","text":"Build failed"}]}`, "Build failed", canonical.MCP},
		{"mcp mirrored", `{"isError":true,"content":[{"type":"text","text":"{\"message\":\"Not ready\",\"code\":\"busy\"}"}],"structuredContent":{"message":"Not ready","code":"busy"}}`, "Not ready\nCode: busy", canonical.MCP},
		{"json", `{"code":"invalid_request","message":"Parameter is invalid","details":{"parameter":"limit"}}`, "Parameter is invalid\nCode: invalid_request\nDetails: parameter=limit", canonical.JSON},
		{"runtime", `{"code":-32000,"message":"Unavailable","data":{"code":"unavailable","details":{"kind":"configuration","data":{"credential":"private"}}}}`, "Unavailable", canonical.MCPError},
	} {
		t.Run(row.name, func(t *testing.T) {
			cache := Cache{}
			projected := cache.Project([]canonical.Record{failedRecord(row.body, row.format)})
			got := string(onlyResponse(&projected[0]).Result.Value)
			if strings.Contains(got, "private") || !strings.Contains(got, strings.ReplaceAll(row.want, "\n", `\n`)) {
				t.Fatalf("failure projection = %s, want %q", got, row.want)
			}
		})
	}
}

func TestFailuresWithContentKeepTheirBodyAndAttachments(t *testing.T) {
	attachment := canonical.Attachment{}
	for _, row := range []struct {
		name, body, keep string
		format           canonical.ToolFormat
		attachments      []canonical.Attachment
	}{
		{"structured details", `{"isError":true,"content":[{"type":"text","text":"3 tests failed"}],"structuredContent":{"message":"3 tests failed","failures":[{"test":"TestA"}]}}`, "TestA", canonical.MCP, nil},
		{"image", `{"isError":true,"content":[{"type":"text","text":"Screenshot differs"},{"type":"image","data":"AAAA","mimeType":"image/png"}]}`, `"image"`, canonical.MCP, nil},
		{"json fields", `{"message":"3 tests failed","failures":["TestB"]}`, "TestB", canonical.JSON, nil},
		{"attachment", `{"isError":true,"content":[{"type":"text","text":"Render failed"}]}`, "Render failed", canonical.MCP, []canonical.Attachment{attachment}},
	} {
		t.Run(row.name, func(t *testing.T) {
			cache := Cache{}
			record := failedRecord(row.body, row.format, row.attachments...)
			projected := cache.Project([]canonical.Record{record})
			response := onlyResponse(&projected[0])
			got := string(response.Result.Value)
			if !strings.Contains(got, row.keep) || response.Status != canonical.ToolErrorStatus ||
				row.format == canonical.MCP && !strings.Contains(got, `"isError":true`) {
				t.Fatalf("failure lost its content: %s", got)
			}
			if len(response.Attachments) != len(row.attachments) {
				t.Fatalf("failure lost its attachments: %+v", response.Attachments)
			}
		})
	}
}

func TestLargeFailureBodyIsBoundedWithSource(t *testing.T) {
	body := encoded(map[string]any{"isError": true, "content": []any{map[string]any{"type": "text", "text": "Suite failed"}},
		"structuredContent": map[string]any{"message": "Suite failed", "log": strings.Repeat("line\n", MaxLines*2)}})
	cache := Cache{}
	projected := cache.Project([]canonical.Record{failedRecord(body, canonical.MCP)})
	got := string(onlyResponse(&projected[0]).Result.Value)
	if len(got) > MaxBytes*2 || !strings.Contains(got, "failed") || !strings.Contains(got, `"isError":true`) {
		t.Fatalf("large failure was not bounded: %d bytes", len(got))
	}
}
