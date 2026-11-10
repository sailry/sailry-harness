// Command computerhelper stands in for the packaged computer helper in the
// B13 integration tests. It answers `permissions [--request]` with the
// helper's JSON envelope, refuses `session` commands and serves two
// `desktop_*` tools over MCP stdio. It never touches the screen, the pointer,
// the keyboard or the system's privacy settings: its grants are the content
// of state.json beside the executable, and every invocation and tool call is
// appended to calls.log there.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	sdk "github.com/modelcontextprotocol/go-sdk/mcp"
)

type grants struct {
	Accessibility   string `json:"accessibility"`
	ScreenRecording string `json:"screen_recording"`
}

func directory() string {
	executable, err := os.Executable()
	if err != nil {
		os.Exit(70)
	}
	return filepath.Dir(executable)
}

func record(line string) {
	file, err := os.OpenFile(filepath.Join(directory(), "calls.log"), os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
	if err != nil {
		os.Exit(71)
	}
	defer file.Close()
	fmt.Fprintln(file, line)
}

func main() {
	args := os.Args[1:]
	record("run " + strings.Join(args, " "))
	if len(args) == 0 {
		os.Exit(64)
	}
	switch args[0] {
	case "permissions":
		path := filepath.Join(directory(), "state.json")
		state := grants{Accessibility: "denied", ScreenRecording: "denied"}
		if body, err := os.ReadFile(path); err == nil {
			_ = json.Unmarshal(body, &state)
		}
		if state.Accessibility == "malformed" {
			fmt.Println("not json")
			return
		}
		if len(args) > 1 && args[1] == "--request" {
			// The stand-in for the system prompt: every refused grant
			// becomes granted, as if the user had allowed it.
			if state.Accessibility == "denied" {
				state.Accessibility = "granted"
			}
			if state.ScreenRecording == "denied" {
				state.ScreenRecording = "granted"
			}
			body, _ := json.Marshal(state)
			if err := os.WriteFile(path, body, 0o600); err != nil {
				os.Exit(72)
			}
		}
		_ = json.NewEncoder(os.Stdout).Encode(map[string]any{"ok": true, "command": "permissions", "data": map[string]any{
			"accessibility": map[string]string{"state": state.Accessibility}, "screen_recording": map[string]string{"state": state.ScreenRecording},
		}})
	case "session":
		_ = json.NewEncoder(os.Stdout).Encode(map[string]any{"ok": false, "command": "session", "error": map[string]string{"code": "UNSUPPORTED"}})
		os.Exit(1)
	case "mcp":
		server := sdk.NewServer(&sdk.Implementation{Name: "computer-helper-fixture", Version: "1"}, nil)
		type input struct {
			Ref string `json:"ref_id,omitempty"`
		}
		tool := func(name string, readOnly bool) {
			sdk.AddTool(server, &sdk.Tool{Name: name, Description: "Fixture " + name, Annotations: &sdk.ToolAnnotations{ReadOnlyHint: readOnly}},
				func(_ context.Context, _ *sdk.CallToolRequest, value input) (*sdk.CallToolResult, any, error) {
					record("tool " + name + " " + value.Ref)
					return &sdk.CallToolResult{Content: []sdk.Content{&sdk.TextContent{Text: "fixture:" + name + ":" + value.Ref}}}, nil, nil
				})
		}
		tool("desktop_snapshot", true)
		tool("desktop_click", false)
		if err := server.Run(context.Background(), &sdk.StdioTransport{}); err != nil {
			os.Exit(1)
		}
	default:
		os.Exit(64)
	}
}
