package protocol

import (
	"encoding/json"
	"testing"
)

func TestToolErrorMessage(t *testing.T) {
	for _, row := range []struct {
		name, payload, want string
	}{
		{"rpc", `{"code":-32000,"message":" Review is unavailable ","data":{"code":"unavailable","details":{"context":"large"}}}`, "Review is unavailable"},
		{"native", `{"isError":true,"error":{"code":"not_configured","message":"Connect the account"}}`, "Connect the account"},
		{"structured", `{"structuredContent":{"message":"Bad path"},"content":[]}`, "Bad path"},
		{"embedded", `{"content":[{"type":"text","text":"{\"error\":{\"message\":\"No such file\"}}"}]}`, "No such file"},
		{"plain MCP", `{"content":[{"type":"text","text":"File was removed"}]}`, "File was removed"},
		{"multiple reasons", `{"content":[{"type":"text","text":"{\"message\":\"First reason\"}"},{"type":"text","text":"{\"message\":\"Second reason\"}"}]}`, ""},
		{"missing reason", `{"content":[{"type":"text","text":"{\"status\":400}"}]}`, ""},
		{"unrelated message", `{"items":[{"message":"A record"}]}`, ""},
	} {
		t.Run(row.name, func(t *testing.T) {
			if got := ToolErrorMessage(json.RawMessage(row.payload)); got != row.want {
				t.Fatalf("ToolErrorMessage = %q, want %q", got, row.want)
			}
		})
	}
}
