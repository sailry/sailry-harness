package plugins

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProgramDeclarations(t *testing.T) {
	for _, current := range []struct {
		name   string
		change func(*protocol.PluginExtension)
		valid  bool
	}{
		{"agent adapter over a declared program", func(*protocol.PluginExtension) {}, true},
		{"program without its action", func(extension *protocol.PluginExtension) {
			extension.Actions = []protocol.PluginAction{protocol.PluginActionProvideModels}
		}, false},
		{"command that is a path", func(extension *protocol.PluginExtension) { extension.Programs[0].Command = "bin/agent" }, false},
		{"command that is a shell line", func(extension *protocol.PluginExtension) { extension.Programs[0].Command = "agent acp" }, false},
		{"argument with an unknown placeholder", func(extension *protocol.PluginExtension) {
			extension.Programs[0].Args = []string{"acp", "{home}"}
		}, false},
		{"key in an argument", func(extension *protocol.PluginExtension) { extension.Programs[0].Args = []string{"acp", "{key}"} }, false},
		{"key in the declared environment", func(extension *protocol.PluginExtension) {
			extension.Programs[0].Env = map[string]string{"AGENT_KEY": "{key}"}
		}, true},
		{"variable the Node owns", func(extension *protocol.PluginExtension) {
			extension.Programs[0].Env = map[string]string{"PATH": "/tmp"}
		}, false},
		{"file outside the program directory", func(extension *protocol.PluginExtension) {
			extension.Programs[0].Files = []protocol.PluginProgramFile{{Path: "../agent.md", Content: "x"}}
		}, false},
		{"absolute file", func(extension *protocol.PluginExtension) {
			extension.Programs[0].Files = []protocol.PluginProgramFile{{Path: "/tmp/agent.md", Content: "x"}}
		}, false},
		{"duplicate program", func(extension *protocol.PluginExtension) {
			extension.Programs = append(extension.Programs, extension.Programs[0])
		}, false},
		{"more programs than the limit", func(extension *protocol.PluginExtension) {
			for _, id := range []string{"b", "c", "d", "e"} {
				other := extension.Programs[0]
				other.ID = id
				extension.Programs = append(extension.Programs, other)
			}
		}, false},
		{"adapter naming an undeclared program", func(extension *protocol.PluginExtension) { extension.ModelAdapters[0].Acp.Program = "missing" }, false},
		{"adapter with a script and a program", func(extension *protocol.PluginExtension) {
			extension.ModelAdapters[0].Script.Entry = "dev.sailry.platform/adapter.js"
		}, false},
		{"adapter with neither", func(extension *protocol.PluginExtension) { extension.ModelAdapters[0].Acp = nil }, false},
		{"adapter with HTTP origins", func(extension *protocol.PluginExtension) {
			extension.ModelAdapters[0].Origins = []string{"https://example.test"}
		}, false},
		{"adapter without a sign-in instruction", func(extension *protocol.PluginExtension) { extension.ModelAdapters[0].Acp.SignIn = "" }, false},
		{"replay budget below the floor", func(extension *protocol.PluginExtension) { extension.ModelAdapters[0].Acp.ReplayBytes = 1024 }, false},
		{"unknown effort level", func(extension *protocol.PluginExtension) {
			extension.ModelAdapters[0].Acp.Efforts = map[string]string{"extreme": "-x"}
		}, false},
		{"metadata path with an empty name", func(extension *protocol.PluginExtension) {
			extension.ModelAdapters[0].Acp.SystemMeta = new("vendor..append")
		}, false},
		{"model argument without its placeholder", func(extension *protocol.PluginExtension) { extension.ModelAdapters[0].Acp.ModelArgument = true }, false},
		{"model placeholder without the model argument", func(extension *protocol.PluginExtension) {
			extension.Programs[0].Args = []string{"acp", "--model", "{model}"}
		}, false},
		{"model chosen by a start argument", func(extension *protocol.PluginExtension) {
			extension.Programs[0].Args = []string{"acp", "--model", "{model}"}
			extension.ModelAdapters[0].Acp.ModelArgument, extension.ModelAdapters[0].Acp.SystemMeta = true, nil
			extension.ModelAdapters[0].Acp.AuthMethod = new("vendor_login")
		}, true},
	} {
		t.Run(current.name, func(t *testing.T) {
			var extension protocol.PluginExtension
			if err := json.Unmarshal([]byte(`{
				"api_version":"v1","scope":"host","actions":["models.provide","process.run"],
				"programs":[{"id":"agent","command":"agent","args":["acp","--dir","{dir}"],"env":{"AGENT_UPDATE":"false"},
					"files":[{"path":".agents/agents/plain/agent.md","content":"plain"}]}],
				"model_adapters":[{"id":"agent","origins":[],"framing":"ndjson","capabilities":{"tools":true,"reasoning":true,"vision":true},
					"acp":{"program":"agent","system_meta":"vendor.systemPrompt.append","efforts":{"low":"-low","high":"-high"},
						"replay_bytes":65536,"sign_in":"Run agent once to sign in","failure_markers":["Agent execution terminated"]}}]
			}`), &extension); err != nil {
				t.Fatal(err)
			}
			current.change(&extension)
			if err := validateExtension(extension); (err == nil) != current.valid {
				t.Fatalf("extension declaration: %v", err)
			}
		})
	}
	// An authored declaration is strict: an unknown member is refused.
	var adapter protocol.PluginModelAdapterDeclaration
	if err := json.Unmarshal([]byte(`{"id":"agent","origins":[],"framing":"ndjson","acp":{"program":"agent","replay_bytes":65536,"sign_in":"x","shell":true}}`), &adapter); err == nil ||
		!strings.Contains(err.Error(), "shell") {
		t.Fatalf("unknown member: %v", err)
	}
}
