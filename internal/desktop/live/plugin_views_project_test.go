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

	t.Run("replaced project controls are not mounted", func(t *testing.T) {
		legacy := info("commands", protocol.PluginScopeHost,
			control("terminal-new", protocol.PluginUiSlotProject), control("terminal-cli", protocol.PluginUiSlotProject),
			control("terminal-menu-new", protocol.PluginUiSlotProjectMenu), control("terminal-menu-cli", protocol.PluginUiSlotProjectMenu))
		legacy.Extension.Desktop.Renderers = []protocol.PluginDesktopRenderer{{Resource: protocol.PluginDesktopResourceKindTerminal, Create: new("terminal-new")}}
		competing := info("other", protocol.PluginScopeHost, control("create-shell", protocol.PluginUiSlotProjectMenu))
		competing.Extension.Desktop.Renderers = []protocol.PluginDesktopRenderer{{Resource: protocol.PluginDesktopResourceKindTerminal, Create: new("create-shell")}}
		worktrees := info("worktrees", protocol.PluginScopeHost, control("manage", protocol.PluginUiSlotProjectMenu))
		for _, current := range []protocol.PluginInfo{legacy, competing, worktrees} {
			if shown := pluginViewDeclarations(current, current.Extension.Ui, "project"); len(shown) != 0 {
				t.Fatalf("%s project controls = %+v", current.Summary.Name, shown)
			}
			if projectMenuDeclared([]protocol.PluginInfo{current}) {
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
		shown := pluginViewDeclarations(deploy, declarations, "project")
		if len(shown) != 2 || shown[0].ID != "manage" || shown[1].ID != "open" {
			t.Fatalf("project controls = %+v", shown)
		}
		if shown := pluginViewDeclarations(deploy, declarations, "composer"); len(shown) != 1 || shown[0].ID != "status" {
			t.Fatalf("composer controls = %+v", shown)
		}
		if !projectMenuDeclared([]protocol.PluginInfo{deploy}) {
			t.Fatal("a contributed project menu item mounts no registry")
		}
	})

	t.Run("a package named commands keeps unrelated project controls", func(t *testing.T) {
		current := info("commands", protocol.PluginScopeHost, control("terminal-menu-new", protocol.PluginUiSlotProjectMenu))
		if !projectMenuDeclared([]protocol.PluginInfo{current}) {
			t.Fatal("a package's name alone reserved its project control")
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
