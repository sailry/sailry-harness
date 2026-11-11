package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProjectPluginControls(t *testing.T) {
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

	t.Run("the bundled controls the application draws itself are not mounted", func(t *testing.T) {
		for name, ids := range map[string][]string{
			"commands":  {"terminal-new", "terminal-cli", "terminal-menu-new", "terminal-menu-cli"},
			"worktrees": {"manage"},
		} {
			var declarations []protocol.PluginUiContribution
			for index, id := range ids {
				slot := protocol.PluginUiSlotProject
				if index%2 == 1 || name == "worktrees" {
					slot = protocol.PluginUiSlotProjectMenu
				}
				declarations = append(declarations, control(id, slot))
			}
			if shown := pluginViewDeclarations(name, declarations, "project"); len(shown) != 0 {
				t.Fatalf("%s project controls = %+v", name, shown)
			}
			if projectMenuDeclared([]protocol.PluginInfo{info(name, protocol.PluginScopeHost, declarations...)}) {
				t.Fatalf("%s keeps a project registry mounted", name)
			}
		}
	})

	t.Run("another package's project controls are drawn", func(t *testing.T) {
		declarations := []protocol.PluginUiContribution{
			control("manage", protocol.PluginUiSlotProjectMenu), control("open", protocol.PluginUiSlotProject),
			control("status", protocol.PluginUiSlotHeader),
		}
		shown := pluginViewDeclarations("deploy", declarations, "project")
		if len(shown) != 2 || shown[0].ID != "manage" || shown[1].ID != "open" {
			t.Fatalf("project controls = %+v", shown)
		}
		if shown := pluginViewDeclarations("commands", declarations, "composer"); len(shown) != 1 || shown[0].ID != "status" {
			t.Fatalf("composer controls = %+v", shown)
		}
		if !projectMenuDeclared([]protocol.PluginInfo{info("deploy", protocol.PluginScopeHost, declarations...)}) {
			t.Fatal("a contributed project menu item mounts no registry")
		}
	})

	t.Run("a package that cannot mount keeps no project registry", func(t *testing.T) {
		item := control("deploy", protocol.PluginUiSlotProjectMenu)
		disabled := info("deploy", protocol.PluginScopeHost, item)
		disabled.Summary.Enabled = false
		button := info("deploy", protocol.PluginScopeHost, control("open", protocol.PluginUiSlotProject))
		for name, current := range map[string]protocol.PluginInfo{
			"disabled": disabled, "desktop scoped": info("deploy", protocol.PluginScopeDesktop, item),
			"overview only": button, "not mounted": info("git", protocol.PluginScopeHost, item),
		} {
			if projectMenuDeclared([]protocol.PluginInfo{current}) {
				t.Fatalf("%s package keeps a project registry mounted", name)
			}
		}
	})
}
