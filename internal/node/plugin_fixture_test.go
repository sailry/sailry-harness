package node

import (
	"context"
	"database/sql"
	"io/fs"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func pluginFixturePath(t *testing.T, name string) string {
	t.Helper()
	path, err := filepath.Abs(filepath.Join("testdata", "plugins", name))
	if err != nil {
		t.Fatal(err)
	}
	return path
}

func pluginFixtureBundles() fs.FS { return os.DirFS("testdata") }

func installPluginFixture(t *testing.T, node *Node, name string) protocol.PluginInfo {
	t.Helper()
	info, err := node.plugins.manager.Install(node.pluginContext(t.Context()), pluginFixturePath(t, name), "", name)
	if err != nil || info.Summary.Name != name || info.Summary.Compatibility.Kind != "compatible" || len(info.Issues) != 0 {
		t.Fatalf("fixture %s: %+v, %v", name, info, err)
	}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		info, err = store.InstallPlugin(t.Context(), db, info, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return info
}

func startBundledFixtureNode(ctx context.Context, path string, defaults ...plugins.DefaultPackage) (*Node, error) {
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	assembly := fixtureModuleAssembly()
	assembly.BundledDefaults = defaults
	assembly.BundledPackages = pluginFixtureBundles()
	return StartWithOptions(ctx, path, Options{
		Assembly: assembly, Environment: &environment, MCP: fixtureMCPOptions(),
	})
}

func bundledFixtureNode(t *testing.T, defaults ...plugins.DefaultPackage) *Node {
	t.Helper()
	node, err := startBundledFixtureNode(t.Context(), filepath.Join(t.TempDir(), "profile"), defaults...)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := node.Stop(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return node
}
