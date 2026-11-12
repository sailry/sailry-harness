package node

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The bundled claude package makes a channel that the Node serves with the
// Claude Code route. No process is started: discovery only looks for the
// executable, and the model is built without a request.
func TestClaudeCodeChannel(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("the stand-in executable is a mode bit")
	}
	bin := t.TempDir()
	environment := process.NewEnvironment([]string{"PATH=" + bin})
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), Options{Environment: &environment})
	if err != nil {
		t.Fatal(err)
	}
	defer node.Stop(context.Background())
	commandFixture(t, node, protocol.Command{Kind: "install_bundled_plugin", Data: protocol.CommandInstallBundledPluginPayload{Name: "claude"}})

	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = protocol.NewProviderID(), "Claude Code", true
	provider.Api, provider.Authentication, provider.Endpoint = protocol.ConversationModelApiPlugin, protocol.AuthenticationApiKey, "https://claude.ai"
	provider.Preset = &protocol.ConversationPresetsPresetRef{ID: "claude-code",
		Source: protocol.ConversationPresetsSource{Kind: "package", Data: protocol.ConversationPresetsSourcePackagePayload{Name: "claude"}}}
	output := commandFixture(t, node, protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider}})
	saved, ok := dispatchPayload[protocol.ConversationProvider](output.Data)
	if !ok || !model.PluginBound(saved) || (**saved.Binding).Adapter.Adapter != "claude" {
		t.Fatalf("saved channel is not bound to the package adapter: %+v", saved.Binding)
	}
	discover := protocol.NewRequest(node.ID(), protocol.Command{Kind: "discover_models", Data: protocol.ConversationDiscoverySource{Kind: "saved",
		Data: protocol.ConversationDiscoverySourceSavedPayload{Provider: saved.ID, ExpectedRevision: saved.Revision}}})

	// Without the executable the channel says so; it lists nothing.
	missing := dispatchFixture(t, node, node.ID(), discover)
	if missing.Fault == nil || !strings.Contains(missing.Fault.Message, "claude is not installed") {
		t.Fatalf("discovery without claude: %+v, %+v", missing.Fault, missing.Output)
	}

	// The stand-in would fail if anything ran it.
	if err := os.WriteFile(filepath.Join(bin, "claude"), []byte("#!/bin/sh\nexit 9\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	discover.ID = protocol.NewRequestID()
	listed := dispatchFixture(t, node, node.ID(), discover)
	if listed.Fault != nil {
		t.Fatal(listed.Fault)
	}
	catalog, ok := dispatchPayload[protocol.ConversationDiscoveryCatalog](listed.Output.Data)
	if !ok || len(catalog.Models) != 3 || catalog.Models[0].ID != "opus" || catalog.Models[0].Context != nil {
		t.Fatalf("catalog = %+v", listed.Output.Data)
	}

	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model = saved.ID, "opus"
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
	binding, err := model.Capture(saved, config, nil, protocol.NewSessionID())
	if err != nil {
		t.Fatal(err)
	}
	language, err := node.newModel(t.Context(), binding)
	if err != nil {
		t.Fatal(err)
	}
	if language.Provider() != "sailry-plugin" || language.Model() != "opus" {
		t.Fatalf("model = %s %s", language.Provider(), language.Model())
	}
	// The route is the Claude Code one, not the agent protocol: it does not
	// offer structured generation and names no script.
	if _, err := language.GenerateObject(t.Context(), fantasy.ObjectCall{}); err == nil {
		t.Fatal("structured generation was accepted")
	} else if failure := new(model.Failure); !errors.As(err, &failure) || failure.Kind != model.Unsupported {
		t.Fatalf("structured generation: %v", err)
	}

	// The terminal interface is the user's to turn on in the package's settings.
	terminal := func() bool {
		t.Helper()
		captured, bound, err := node.captureProgramModel(t.Context(), binding)
		if err != nil || !bound {
			t.Fatalf("capture: %v, %v", bound, err)
		}
		capture, err := node.claudeCapture(t.Context(), captured)
		if err != nil {
			t.Fatal(err)
		}
		return capture.Interactive
	}
	if terminal() {
		t.Fatal("the terminal interface is on by default")
	}
	commandFixture(t, node, protocol.Command{Kind: "save_plugin_settings", Data: protocol.CommandSavePluginSettingsPayload{
		Package: *(**saved.Binding).Package, Values: map[string]json.RawMessage{"terminal": json.RawMessage(`true`)},
		Secrets: map[string]protocol.PluginSettingsSecretUpdate{},
	}})
	if !terminal() {
		t.Fatal("the saved setting did not turn the terminal interface on")
	}
}
