package live

import (
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func TestProjectPluginControls(t *testing.T) {
	r := &Runtime{pluginControlReplacements: testassembly.Full().PluginControlReplacements}
	shownOn := func(info protocol.PluginInfo, surface string) []protocol.PluginUiContribution {
		var shown []protocol.PluginUiContribution
		for _, declaration := range info.Extension.Ui {
			if r.pluginViewShows(info, declaration, surface) {
				shown = append(shown, declaration)
			}
		}
		return shown
	}
	entry := "ui.js"
	info := func(name string, scope protocol.PluginScope, declarations ...protocol.PluginUiContribution) protocol.PluginInfo {
		return protocol.PluginInfo{
			Summary: protocol.PluginSummary{Name: name, Enabled: true, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}},
			Extension: &protocol.PluginExtension{
				Scope: scope, Desktop: &protocol.PluginDesktopManifest{UiEntry: &entry}, Ui: declarations,
			},
		}
	}
	control := func(id string, slot protocol.PluginUiSlot) protocol.PluginUiContribution {
		return protocol.PluginUiContribution{ID: id, Slot: slot, Kind: "button"}
	}

	t.Run("replaced project controls are not mounted", func(t *testing.T) {
		legacy := info("commands", protocol.PluginScopeHost,
			control("terminal-new", protocol.PluginUiSlotProject), control("terminal-cli", protocol.PluginUiSlotProject),
			control("terminal-menu-new", protocol.PluginUiSlotProjectMenu), control("terminal-menu-cli", protocol.PluginUiSlotProjectMenu))
		legacy.Extension.Desktop.Renderers = []protocol.PluginDesktopRenderer{{Resource: protocol.PluginDesktopResourceKindTerminal, Create: new("terminal-new")}}
		legacy.Summary.Digest = "21acc4f71cc906427940f044b83ea4a59e639bcaacfae1dedef3f26e4252cd67"
		competing := info("other", protocol.PluginScopeHost, control("create-shell", protocol.PluginUiSlotProject))
		competing.Extension.Desktop.Renderers = []protocol.PluginDesktopRenderer{{Resource: protocol.PluginDesktopResourceKindTerminal, Create: new("create-shell")}}
		worktrees := info("worktrees", protocol.PluginScopeHost, control("manage", protocol.PluginUiSlotProjectMenu))
		worktrees.Summary.Digest = "59b430e3b3df231f2caf1457f594ba41698cca1ff5342908ba436077e4d58ba1"
		for _, current := range []protocol.PluginInfo{legacy, competing, worktrees} {
			if shown := shownOn(current, "project"); len(shown) != 0 {
				t.Fatalf("%s project controls = %+v", current.Summary.Name, shown)
			}
			if r.projectMenuDeclared([]protocol.PluginInfo{current}) {
				t.Fatalf("%s keeps a project registry mounted", current.Summary.Name)
			}
		}
	})

	t.Run("another package's project controls are drawn", func(t *testing.T) {
		declarations := []protocol.PluginUiContribution{
			control("manage", protocol.PluginUiSlotProjectMenu), control("open", protocol.PluginUiSlotProject),
			control("status", protocol.PluginUiSlotHeader),
		}
		deploy := info("deploy", protocol.PluginScopeHost, declarations...)
		shown := shownOn(deploy, "project")
		if len(shown) != 2 || shown[0].ID != "manage" || shown[1].ID != "open" {
			t.Fatalf("project controls = %+v", shown)
		}
		if shown := shownOn(deploy, "composer"); len(shown) != 1 || shown[0].ID != "status" {
			t.Fatalf("composer controls = %+v", shown)
		}
		if !r.projectMenuDeclared([]protocol.PluginInfo{deploy}) {
			t.Fatal("a contributed project menu item mounts no registry")
		}
	})

	t.Run("same-name packages keep project controls unlinked to unsupported renderers", func(t *testing.T) {
		current := info("commands", protocol.PluginScopeHost, control("terminal-menu-new", protocol.PluginUiSlotProjectMenu))
		current.Extension.Desktop.Renderers = []protocol.PluginDesktopRenderer{{Resource: protocol.PluginDesktopResourceKindTerminal, Create: new("other-launcher")}}
		for _, pkg := range []protocol.PluginInfo{
			current, info("worktrees", protocol.PluginScopeHost, control("manage", protocol.PluginUiSlotProjectMenu)),
			info("git", protocol.PluginScopeHost, control("manage", protocol.PluginUiSlotProjectMenu)), info("files", protocol.PluginScopeHost, control("manage", protocol.PluginUiSlotProjectMenu)),
		} {
			if !r.projectMenuDeclared([]protocol.PluginInfo{pkg}) {
				t.Fatalf("a package's name alone reserved its project control: %s", pkg.Summary.Name)
			}
		}
	})

	t.Run("a package that cannot mount keeps no project registry", func(t *testing.T) {
		item := control("deploy", protocol.PluginUiSlotProjectMenu)
		disabled := info("deploy", protocol.PluginScopeHost, item)
		disabled.Summary.Enabled = false
		button := info("deploy", protocol.PluginScopeHost, control("open", protocol.PluginUiSlotProject))
		retired := info("git", protocol.PluginScopeHost, item)
		retired.Summary.Digest = "c878628617b254c50d7d493e6dbf7e68a343499e4882cf712d213e65c26478d4"
		for name, current := range map[string]protocol.PluginInfo{
			"disabled": disabled, "desktop scoped": info("deploy", protocol.PluginScopeDesktop, item),
			"overview only": button, "historical": retired,
		} {
			if r.projectMenuDeclared([]protocol.PluginInfo{current}) {
				t.Fatalf("%s package keeps a project registry mounted", name)
			}
		}
	})
}

func TestLegacyPluginControlsFollowAssembly(t *testing.T) {
	worktrees := protocol.PluginReference{Name: "worktrees", Digest: "ae42d37635d7990e29b1654a311ac5ace70e47b9a7f1226721a63482406fbb9e"}
	commands := protocol.PluginReference{Name: "commands", Digest: "21acc4f71cc906427940f044b83ea4a59e639bcaacfae1dedef3f26e4252cd67"}
	for _, row := range []struct {
		name               string
		modules            []shell.Module
		terminal, worktree bool
	}{
		{name: "empty"},
		{name: "unrelated terminal registration", modules: []shell.Module{native.RegisterTerminal}},
		{name: "terminal only", modules: []shell.Module{testassembly.RegisterTerminalFixture}, terminal: true},
		{name: "worktree only", modules: []shell.Module{testassembly.RegisterWorktreeFixture}, worktree: true},
		{name: "combined", modules: []shell.Module{testassembly.RegisterWorktreeFixture, testassembly.RegisterTerminalFixture}, terminal: true, worktree: true},
	} {
		t.Run(row.name, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			controller, _ := optionalDesktopController(t, snapshot)
			root, err := NewWithAssembly(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "prefs.json")), shell.Assemble(row.modules...))
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = root.Close() })
			for _, check := range []struct {
				ref      protocol.PluginReference
				slot     protocol.PluginUiSlot
				id       string
				replaced bool
			}{
				{worktrees, protocol.PluginUiSlotContext, "location", true},
				{worktrees, protocol.PluginUiSlotComposer, "create", row.worktree},
				{worktrees, protocol.PluginUiSlotComposer, "fork", row.worktree},
				{worktrees, protocol.PluginUiSlotProjectMenu, "manage", row.worktree},
				{worktrees, protocol.PluginUiSlotProject, "manage", false},
				{commands, protocol.PluginUiSlotProject, "terminal-cli", row.terminal},
				{commands, protocol.PluginUiSlotProjectMenu, "terminal-menu-new", row.terminal},
				{commands, protocol.PluginUiSlotProjectMenu, "terminal-menu-cli", row.terminal},
				{commands, protocol.PluginUiSlotProjectMenu, "terminal-cli", false},
			} {
				declaration := protocol.PluginUiContribution{ID: check.id, Slot: check.slot}
				if got := root.legacyPluginControlReplaced(check.ref, declaration); got != check.replaced {
					t.Errorf("%s/%s/%s replaced = %v, want %v", check.ref.Name, check.slot, check.id, got, check.replaced)
				}
				unrelated := check.ref
				unrelated.Digest = strings.Repeat("f", 64)
				if root.legacyPluginControlReplaced(unrelated, declaration) {
					t.Errorf("same-name package lost %s/%s", check.slot, check.id)
				}
			}
		})
	}
}
