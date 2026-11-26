package application

import (
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"testing"

	hostassembly "github.com/sailry/sailry-harness/internal/assembly/host"
	"github.com/sailry/sailry-harness/internal/desktop/testplugin"
	"github.com/sailry/sailry-harness/internal/identity"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHarnessDesktopHostPreservesExplicitPluginChoices(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "profile")
	start := func() *desktopBackend {
		t.Helper()
		backend, err := openBackend(t.Context(), startupOptions{DataDir: profile}, identity.DefaultDesktop(), hostassembly.Harness())
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := backend.Close(); err != nil {
				t.Error(err)
			}
		})
		if backend.Path != profile || backend.Node == nil || backend.Client == nil || !backend.Client.Connected() {
			t.Fatalf("Harness Desktop did not start its real Host in the temporary profile: %+v", backend)
		}
		return backend
	}
	backend := start()
	if inventory := backend.Client.Snapshot().Plugins; len(inventory) != 0 {
		t.Fatalf("a fresh Harness Desktop Host installed plugins: %+v", inventory)
	}
	do := func(command protocol.Command) protocol.Output {
		t.Helper()
		output, err := backend.Client.Do(t.Context(), command)
		if err != nil {
			t.Fatalf("%s: %v", command.Kind, err)
		}
		return output
	}

	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	const name, text = "notes.txt", "Native files without plugins.\n"
	if err := os.WriteFile(filepath.Join(root, name), []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
	project := do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Harness files", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance(),
	}}).Data.(protocol.Project)
	snapshot, err := backend.Node.Snapshot(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	index := slices.IndexFunc(snapshot.Worktrees, func(tree protocol.Worktree) bool {
		return tree.Project != nil && *tree.Project == project.ID && tree.Main
	})
	if index < 0 {
		t.Fatal("the Host did not create the project's main worktree")
	}
	worktree := snapshot.Worktrees[index].ID
	read := func() {
		t.Helper()
		file := do(protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: worktree, Path: name}}).Data.(protocol.FileContent)
		if file.Text != text || file.Revision == nil {
			t.Fatalf("Harness Desktop Host could not read a native file without the files plugin: %+v", file)
		}
	}
	read()
	if inventory := backend.Client.Snapshot().Plugins; len(inventory) != 0 {
		t.Fatalf("using native files installed plugins: %+v", inventory)
	}

	selected, err := testplugin.Install(t.Context(), backend.Client, testplugin.Basic("retained-fixture"))
	if err != nil {
		t.Fatal(err)
	}
	if selected.Summary.Name != "retained-fixture" || !selected.Summary.Enabled || selected.Summary.Compatibility.Kind != "compatible" {
		t.Fatalf("explicit Desktop plugin installation = %+v", selected.Summary)
	}
	selected = do(protocol.Command{Kind: "set_plugin_enabled", Data: protocol.CommandSetPluginEnabledPayload{
		Name: selected.Summary.Name, ExpectedRevision: selected.Summary.Revision, Enabled: false,
	}}).Data.(protocol.PluginInfo)
	selected = do(protocol.Command{Kind: "set_plugin_pinned", Data: protocol.CommandSetPluginPinnedPayload{
		Name: selected.Summary.Name, ExpectedRevision: selected.Summary.Revision, Pinned: true,
	}}).Data.(protocol.PluginInfo)
	removed, err := testplugin.Install(t.Context(), backend.Client, testplugin.Basic("removed-fixture"))
	if err != nil {
		t.Fatal(err)
	}
	do(protocol.Command{Kind: "remove_plugin", Data: protocol.CommandRemovePluginPayload{
		Name: removed.Summary.Name, ExpectedRevision: removed.Summary.Revision,
	}})
	if selected.Summary.Enabled || !selected.Summary.Pinned {
		t.Fatalf("Desktop plugin choices were not applied: %+v", selected.Summary)
	}
	before, err := backend.Node.Snapshot(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if len(before.Plugins) != 1 || !reflect.DeepEqual(before.Plugins[0], selected.Summary) {
		t.Fatalf("Desktop inventory did not follow the explicit choices: %+v", before.Plugins)
	}
	id := backend.Node.ID()
	if err := backend.Close(); err != nil {
		t.Fatal(err)
	}

	backend = start()
	if after := backend.Client.Snapshot(); backend.Node.ID() != id || !reflect.DeepEqual(after.Plugins, before.Plugins) {
		t.Fatalf("Harness Desktop restart changed its Host or plugin choices: before %+v, after %+v", before.Plugins, after.Plugins)
	}
	read()
}
