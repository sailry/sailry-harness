package live

import (
	"os"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
	nodeplugins "github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestGitLegacyDeclarationsFollowAssemblyAndIdentity(t *testing.T) {
	const official = "4d894eb785dca39ae5610df7c0270133ecece5d2fb93705e36603a3182c99aeb"
	controls := []shell.PluginControlReplacement{
		{Package: "git", Slot: "context", ID: "branch"},
		{Package: "git", Slot: "project_menu", ID: "manage"},
		{Package: "git", Slot: "status", ID: "changes"},
	}
	declarations := []protocol.PluginUiContribution{
		{ID: "branch", Slot: "context"},
		{ID: "manage", Slot: "project_menu"},
		{ID: "changes", Slot: "status"},
	}
	digest := official
	if dir := os.Getenv("SAILRY_LEGACY_GIT_DIR"); dir != "" {
		manager := nodeplugins.NewManager(t.TempDir())
		info, err := manager.Install(t.Context(), dir, "", "git")
		if err != nil {
			t.Fatal(err)
		}
		digest = info.Summary.Digest
		if !nodeplugins.IsLegacyDesktopPackage("git", digest) || info.Extension == nil || info.Extension.Desktop == nil {
			t.Fatalf("installed package is not a verified Git Desktop release: %s", digest)
		}
		declarations = info.Extension.Ui
		for _, role := range []string{"workspace", "composer", "project"} {
			bundle, err := manager.ReadDesktop(t.Context(), info, protocol.PluginDesktopSurface{Kind: role})
			if err != nil || bundle.Entry == "" {
				t.Fatalf("real package %s entry: %+v %v", role, bundle.Entry, err)
			}
		}
	}
	if len(declarations) != len(controls) {
		t.Fatalf("actual Git controls=%+v, replacements=%+v", declarations, controls)
	}
	for _, row := range []struct {
		name, digest     string
		native, replaced bool
	}{
		{"legacy without Git module", digest, false, false},
		{"legacy beside Git module", digest, true, true},
		{"unrelated same-name package", strings.Repeat("f", 64), true, false},
	} {
		t.Run(row.name, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
			if row.native {
				root.App = shell.NewLiveWithAssembly(root.App.Data, shell.Assemble(func(a *shell.Assembly) { a.WorkspacePages = append(a.WorkspacePages, legacyViewFixture("git", true)) }))
				root.pluginControlReplacements = controls
			}
			ref := protocol.PluginReference{Name: "git", Digest: row.digest}
			if root.pluginNavigationReplaced(ref) != row.replaced || root.pluginViewsRetired(ref) != row.replaced {
				t.Fatal("desktop routes were not matched by assembly and digest")
			}
			for _, declaration := range declarations {
				if root.legacyPluginControlReplaced(ref, declaration) != row.replaced {
					t.Fatalf("unexpected disposition: %s/%s", declaration.Slot, declaration.ID)
				}
			}
		})
	}
}
