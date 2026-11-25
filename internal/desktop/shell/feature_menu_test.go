package shell

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestFeatureMenuTracksLiveRegistration(t *testing.T) {
	a := NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, Assembly{})
	const id = "plugin:local/example"
	var labels []string
	a.OnFeatureMenuChanged = func() {
		label := ""
		for _, group := range a.Menu("en", func(string) {}) {
			for _, item := range group.Submenu {
				if item.ID == id {
					label = item.Label
				}
			}
		}
		labels = append(labels, label)
	}
	a.SetPluginFeatures([]Feature{{ID: id, Label: "Original name"}})
	a.SetPluginFeatures([]Feature{{ID: id, Label: "Original name"}})
	a.SetPluginFeatures([]Feature{{ID: id, Label: "New name"}})
	a.SetPluginFeatures(nil)
	if !slices.Equal(labels, []string{"Original name", "New name", ""}) {
		t.Fatalf("menu updates: %q", labels)
	}
}
