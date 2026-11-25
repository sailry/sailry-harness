package engine

import (
	"context"
	"database/sql"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCodeMarketingVersionRunsLegacyCapture(t *testing.T) {
	previous := link.ProductVersion
	link.ProductVersion = "0.1.0"
	t.Cleanup(func() { link.ProductVersion = previous })
	m := &scriptedModel{scripts: []script{response(text("Legacy capture completed"), finish())}}
	f := newFixture(t, m)
	source := t.TempDir()
	manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"legacy-platform-fixture","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1","min_sailry_version":"0.2.13"}}}`
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(manifest), 0o600); err != nil {
		t.Fatal(err)
	}
	info, err := plugins.NewManager(f.r.profile).Install(f.ctx, source, "", "legacy-platform-fixture")
	if err != nil {
		t.Fatal(err)
	}
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		if _, err := store.InstallPlugin(f.ctx, db, info, 0, true); err != nil {
			return err
		}
		_, err := db.ExecContext(f.ctx, "UPDATE node SET plugin_defaults=1 WHERE singleton=1")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	profile, frames, publish, timer := f.r.profile, f.r.feed, f.r.publish, f.r.clock
	f.r.cancel()
	want := plugins.Runtime{ProductID: "code", Modules: []string{"files"}}
	seen := make(chan plugins.Runtime, 1)
	f.r = New(f.db.Node, f.node, profile, frames, publish, Options{
		Clock: timer, PluginRuntime: want,
		Model: func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return m, nil },
		ToolsForTurn: func(ctx context.Context, inv *store.AgentInvocation) ([]Tool, error) {
			select {
			case seen <- plugins.RuntimeFromContext(ctx):
			default:
			}
			_, err := operations.CaptureCatalog(ctx, profile, inv.Turn.Plugins)
			return nil, err
		},
	})
	f.start()
	turn := f.submit("Run the captured plugin on Code")
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	select {
	case actual := <-seen:
		if actual.ProductID != want.ProductID || !slices.Equal(actual.Modules, want.Modules) {
			t.Fatalf("background turn lost product assembly: %+v", actual)
		}
	default:
		t.Fatal("the execution capture path did not run")
	}
}
