package node

import (
	"context"
	"database/sql"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDiscoverModelsMissingProgram(t *testing.T) {
	t.Setenv("SAILRY_TELEMETRY", "off")
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(root, "profile")
	closeLog, err := logging.Install(profile, "host")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	environment := process.NewEnvironment([]string{"PATH=" + t.TempDir()})
	node, err := StartWithOptions(t.Context(), profile, Options{Environment: &environment, MCP: fixtureMCPOptions()})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = node.Stop(context.Background()) })

	const name, executable, endpoint = "discovery-fixture", "discovery-fixture-program", "https://discovery-fixture.invalid"
	source := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"` + name + `","version":"0.1.0","extensions":{"dev.sailry.platform":{
		"api_version":"v1","scope":"host","actions":["models.provide","process.run"],
		"programs":[{"id":"agent","command":"` + executable + `","args":[]}],
		"model_adapters":[{"id":"agent","origins":[],"framing":"ndjson","discovery":true,
			"acp":{"program":"agent","replay_bytes":65536,"sign_in":"Sign in before discovery"}}],
		"model_providers":[{"id":"fixture","name":"Fixture","routes":[{"protocol":"plugin","endpoint":"` + endpoint + `"}],
			"auth":{"kind":"none"},"adapter":"agent","models":{"kind":"discover"}}]
	}}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := node.plugins.manager.Install(t.Context(), source, "", name)
	if err != nil || len(info.Issues) != 0 || info.Extension == nil {
		t.Fatalf("discovery fixture: %v, %v", info.Issues, err)
	}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		_, err := store.InstallPlugin(t.Context(), db, info, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}

	secret := protocol.NewSecret("discovery-fixture-private-key")
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "discover_models", Data: protocol.ConversationDiscoverySource{Kind: "draft", Data: protocol.ConversationDiscoveryDraft{
		Provider: protocol.NewProviderID(), Api: protocol.ConversationModelApiPlugin, Endpoint: endpoint, Secret: &secret,
		Preset: &protocol.ConversationPresetsPresetRef{ID: "fixture", Source: protocol.ConversationPresetsSource{Kind: "package", Data: protocol.ConversationPresetsSourcePackagePayload{Name: name}}},
	}}})
	recorded := make(chan logging.Fields, 1)
	logging.SetReport(func(_ slog.Level, fields logging.Fields) {
		if fields["request"] == request.ID.String() {
			select {
			case recorded <- fields:
			default:
			}
		}
	})
	fault := dispatchFixture(t, node, node.ID(), request).Fault
	if fault == nil || fault.Code != protocol.ErrorCodeNotConfigured || !logging.ValidReportRef(fault.ReportRef) {
		t.Fatalf("discovery fault = %+v", fault)
	}
	select {
	case local := <-recorded:
		if local["command"] != "discover_models" || local["report_ref"] != fault.ReportRef || local["command_stage"] != "completion" || local["cause_category"] != "request" {
			t.Fatalf("local discovery record = %v", local)
		}
		remote := logging.Remote(local)
		if remote["operation"] != "discover_models" || remote["report_ref"] != fault.ReportRef || remote["code"] != "not_configured" || remote["cause_category"] != nil || remote["request"] != nil {
			t.Fatalf("remote discovery fields = %v", remote)
		}
	default:
		t.Fatal("discovery failure was not recorded")
	}
	for _, private := range []string{name, executable, endpoint, secret.Expose()} {
		if strings.Contains(fault.Message, private) {
			t.Fatal("discovery fault contained private adapter input")
		}
	}
}
