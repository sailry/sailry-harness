package composer

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/uitest"
)

// A model's row opens its efforts without choosing it; the effort's row
// chooses. A model without efforts is chosen from its row.
func TestModelPickerChoosesAtAClickedEffort(t *testing.T) {
	uitest.Parallel(t)
	ctx := &Context{Models: []Model{
		{ID: "a", Name: "Alpha", Provider: "P", Reasoning: true, Efforts: []string{"low", "high"}, DefaultEffort: "low"},
		{ID: "b", Name: "Beta", Provider: "P", Reasoning: true, Efforts: []string{"low", "medium", "high"}, DefaultEffort: "low"},
		{ID: "c", Name: "Gamma", Provider: "P"},
	}}
	state := New("a")
	state.layer, state.open = layerModels, map[string]bool{"P": true}
	tester := uitest.Fast(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Size(640, 400).Children(func() {
			if state.layer == layerModels {
				state.modelPicker(c, ctx)
			}
		})
	}, 640, 400, "en")
	click := func(name string) {
		t.Helper()
		row, ok := tester.Access(name)
		if !ok {
			t.Fatalf("no %q row: %q", name, tester.Texts())
		}
		tester.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
		tester.Frame()
	}

	if tester.HasText("medium") {
		t.Fatal("the column shows another model's efforts before a click")
	}
	click("Beta")
	if state.Model != "a" || state.layer != layerModels {
		t.Fatalf("a model's row chose it: model %q, layer %v", state.Model, state.layer)
	}
	if !tester.HasText("medium") {
		t.Fatalf("the clicked model's efforts are missing: %q", tester.Texts())
	}
	click("medium")
	if state.Model != "b" || state.Effort != 1 || state.layer != layerMain {
		t.Fatalf("the effort chose model %q at %v, layer %v", state.Model, state.Effort, state.layer)
	}

	state.layer, state.efforts = layerModels, effortState{}
	tester.Frame()
	click("Gamma")
	if state.Model != "c" || state.layer != layerMain {
		t.Fatalf("a model without efforts was not chosen from its row: %q", state.Model)
	}
}
