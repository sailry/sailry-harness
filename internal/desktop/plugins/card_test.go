package plugins

import (
	"bytes"
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testCardHost(t *testing.T) {
	initial := json.RawMessage(`{"label":"Ready","configured":false}`)
	updated := json.RawMessage(`{"label":"Updated","configured":true}`)
	native := NewNativeBinding(t.Context())
	declaration := protocol.DefaultPluginCardDeclaration()
	declaration.McpServer = new("target")
	declaration.Capabilities.Refresh, declaration.Capabilities.Settings = true, true
	declaration.Capabilities.Browser = []protocol.PluginCardTarget{{
		Name: "login", Source: protocol.PluginCardSource{Kind: "mcp_authorization"},
		Modes: []protocol.PluginCardMode{protocol.PluginCardModeExternal},
	}}
	host, err := NewCardHost(CardBindings{
		Context: t.Context(), Native: &native, Declaration: declaration,
		Summary: protocol.PluginCardSummary{Props: initial},
		ValidateProps: func(value json.RawMessage) bool {
			return bytes.Equal(value, initial) || bytes.Equal(value, updated)
		},
		Status: func(context.Context) (protocol.PluginConnectionStatus, error) {
			return protocol.PluginConnectionStatus{
				Package: protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}, Card: "connection", Server: "target",
				Configuration: protocol.PluginConnectionConfigurationReady,
				Authorization: protocol.PluginConnectionAuthorizationConfigured, Credential: protocol.PluginConnectionCredentialPresent,
			}, nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer host.Close()
	modules := host.Modules()
	if len(modules) != 1 || modules[0].Name != "sailry/card" || len(modules[0].Exports) != 6 {
		t.Fatal("card exposed a general package dispatcher")
	}
	exports := map[string]HostExport{}
	for _, export := range modules[0].Exports {
		exports[export.Name] = export
		if export.Name == "prepare" || export.Name == "execute" || export.Name == "action" {
			t.Fatal("card exposed a programmatic user action")
		}
	}
	read, err := exports["state"].Sync(t.Context(), nil)
	if err != nil || strings.Contains(string(read), "authorization_url") {
		t.Fatalf("card initial state was unavailable or retained private fields: %v", err)
	}
	if _, err := exports["publishProps"].Sync(t.Context(), []json.RawMessage{json.RawMessage(`{"authorization_url":"private"}`)}); err == nil {
		t.Fatal("card public properties bypassed their captured schema")
	}
	if _, err := exports["publishProps"].Sync(t.Context(), []json.RawMessage{updated}); err != nil {
		t.Fatal(err)
	}
	state := host.State()
	state.Props[0] = 'x'
	if !json.Valid(host.State().Props) || !bytes.Equal(host.binding.Summary.Props, initial) {
		t.Fatal("card publication changed the immutable receipt or shared state")
	}
	read, err = exports["status"].Async(t.Context(), nil)
	var status map[string]json.RawMessage
	if err != nil || json.Unmarshal(read, &status) != nil || string(status["configured"]) != "true" || !host.State().Configured {
		t.Fatalf("card status omitted its explicit configured projection: %s %v", read, err)
	}
	if _, err := exports["nextChange"].Async(t.Context(), []json.RawMessage{json.RawMessage(`"0"`)}); err != nil {
		t.Fatal(err)
	}
	current := host.State().Cursor
	cancelled, cancel := context.WithCancel(t.Context())
	cancel()
	cursor, _ := json.Marshal(current)
	if _, err := exports["nextChange"].Async(cancelled, []json.RawMessage{cursor}); err == nil {
		t.Fatal("card change observation ignored cancellation")
	}
	for _, action := range []CardAction{
		{Kind: "connect"}, {Kind: "cancel"}, {Kind: "refresh"}, {Kind: "settings"},
		{Kind: "browser", Target: new("login"), Mode: new(protocol.PluginCardModeExternal)},
	} {
		if !host.Permits(action) {
			t.Fatalf("declared card capability was denied: %s", action.Kind)
		}
	}
	for _, action := range []CardAction{
		{Kind: "connect", Target: new("login")}, {Kind: "cancel", Target: new("login")},
		{Kind: "browser", Target: new("login"), Mode: new(protocol.PluginCardModeEmbedded)}, {Kind: "dispatch"},
	} {
		if host.Permits(action) {
			t.Fatalf("undeclared card capability was exposed: %s", action.Kind)
		}
	}
	if encoded, err := encodeCardQr("Sailry card"); err != nil || encoded.image == nil || encoded.width < 116 || encoded.width > 768 {
		t.Fatalf("bounded card QR encoding failed: %v", err)
	}
	for _, invalid := range []string{"", strings.Repeat("x", CardMaxQrBytes+1)} {
		if _, err := encodeCardQr(invalid); err == nil {
			t.Fatal("card QR accepted an invalid payload size")
		}
	}
	if err := host.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := exports["state"].Sync(t.Context(), nil); err == nil {
		t.Fatal("closed card retained an active host module")
	}
}
