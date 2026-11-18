package composer

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestModelFeatureTooltips(t *testing.T) {
	for _, test := range []struct {
		name     string
		feature  Feature
		selected string
		label    string
		tip      string
	}{
		{name: "cycling feature", feature: Feature{ID: "speed", Name: "Speed", Options: []FeatureOption{{ID: "fast"}, {ID: "normal", Name: "Normal"}}},
			label: i18n.In("en", "composer_feature_fast"), tip: i18n.In("en", "composer_feature_off") + " · " + i18n.In("en", "composer_feature_fast")},
		{name: "withdrawn speed option", feature: Feature{ID: "speed", Options: []FeatureOption{{ID: "fast"}}}, selected: "ultrafast",
			label: i18n.In("en", "composer_feature_fast"), tip: i18n.In("en", "composer_feature_fast") + " · " + i18n.In("en", "composer_feature_description")},
		{name: "feature menu", feature: Feature{ID: "format", Name: "Format", Options: []FeatureOption{{ID: "json", Name: "JSON"}, {ID: "text", Name: "Text"}}},
			label: "Format", tip: "Format · " + i18n.In("en", "composer_feature_description")},
	} {
		t.Run(test.name, func(t *testing.T) {
			state := New("fixture")
			if test.selected != "" {
				state.Features = map[string]string{test.feature.ID: test.selected}
			}
			model := Model{Features: []Feature{test.feature}}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Dark)
				ui.Row(c.Key("features")).Padding(80).Gap(8).Children(func() { state.features(c, model) })
			}, 400, 240)
			tester.SetLocale("en")
			tester.Frame()
			button, found := tester.Access(test.label)
			if !found || button.Description != test.tip {
				t.Fatalf("feature lost its label or detailed description: %+v", button)
			}
			tester.Move(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
			tester.Advance(time.Second)
			tester.Frame()
			tips := 0
			for _, element := range tester.AccessAll() {
				if element.Role == ui.RoleTooltip {
					tips++
				}
			}
			if tips != 1 || !tester.HasText(test.tip) {
				t.Fatalf("feature should own one detailed tooltip, got %d: %q", tips, tester.Texts())
			}
			// The tooltip change must preserve the control's keyboard behavior.
			tester.Move(4, 4)
			tester.Key(0, ui.KeyTab)
			if !tester.Focused(test.label) {
				t.Fatal("the feature lost its keyboard focus stop")
			}
			tester.Key(0, ui.KeyEnter)
			tester.Frame()
			if test.feature.ID == "speed" {
				if state.Features["speed"] != "fast" {
					t.Fatal("the feature no longer cycles from the keyboard")
				}
			} else if _, found := tester.Access("JSON"); !found {
				t.Fatal("the feature menu no longer opens from the keyboard")
			}
		})
	}
}
