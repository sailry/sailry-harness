package shell

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestFeatureInheritsVerifiedLocalPreferences(t *testing.T) {
	const page, old, other = "page:example", "plugin:local/old", "plugin:remote/old"
	for _, verified := range []bool{false, true} {
		a := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{WorkspacePages: []NativePage{{
			Page: page, Feature: Feature{ID: page, DefaultPinned: true, LegacyPackage: "old"}, Render: func(*ui.Context, *App) {},
		}}})
		if verified {
			a.InheritPackagePreferences("local", "old")
		}
		order := []string{other, old, "another"}
		a.SetFeaturePreferences(order, map[string]bool{old: false, other: true})
		got, pins := a.FeaturePreferences()
		if pins[page] == verified || pins[old] || !pins[other] || verified != (slices.Index(got, page) == slices.Index(got, old)+1) {
			t.Fatalf("verified %v: order %v pins %v", verified, got, pins)
		}
		prefs := DefaultPrefs()
		prefs.PluginWorkspaces = map[string]PluginWorkspacePrefs{old + "/main": {Width: 345, Folded: true}, other + "/main": {Width: 390}}
		a.SetPrefs(prefs)
		gotPrefs := a.savedPluginWorkspaces()
		if got, exists := gotPrefs[page+"/main"]; exists != verified || exists && got != prefs.PluginWorkspaces[old+"/main"] {
			t.Fatalf("verified %v: layouts %v", verified, gotPrefs)
		}
		prefs.PluginWorkspaces[page+"/main"] = PluginWorkspacePrefs{Width: 420}
		a.SetPrefs(prefs)
		a.SetFeaturePreferences([]string{page, other, old}, map[string]bool{page: true, old: false})
		got, pins = a.FeaturePreferences()
		if !pins[page] || slices.Index(got, page) > slices.Index(got, old) || a.savedPluginWorkspaces()[page+"/main"].Width != 420 || a.savedPluginWorkspaces()[old+"/main"].Width != 345 {
			t.Fatal("an explicit preference or its predecessor was overwritten")
		}
	}
}
