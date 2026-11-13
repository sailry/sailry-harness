package composer_test

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// Two packages each declare a ring on the same side of the toolbar. Each is
// keyed by its own id, so the two are separate elements: both draw, side by
// side, and pressing one opens its panel alone.
func TestIndicators(t *testing.T) {
	const first, second = "plugin:statistics/context", "plugin:budget/spend"
	open := map[string]bool{}
	ring := func(id, hint string, percent float64) composer.Indicator {
		return composer.Indicator{ID: id, Percent: percent, Hint: hint, Open: open[id],
			Toggle: func(shows bool) { open[id] = shows },
			Panel:  func(c *ui.Context) { ui.Text(c, "panel of "+hint) }}
	}
	tt := tray("en", func() *composer.Context {
		return &composer.Context{Activity: composer.Activity{Connected: true}, Indicators: []composer.Indicator{
			ring(first, "Context 25%", 25),
			ring(second, "Budget 60%", 60),
			{ID: "plugin:statistics/plain", Percent: 10, Hint: "Plain ring", Tone: kit.Caution},
			{ID: "plugin:budget/plain", Percent: 90, Hint: "Other plain ring", Tone: kit.Negative},
		}}
	})
	context, found := tt.AccessByRole(ui.RoleButton, "Context 25%")
	budget, also := tt.AccessByRole(ui.RoleButton, "Budget 60%")
	if !found || !also {
		t.Fatalf("the toolbar's rings: context %v, budget %v", found, also)
	}
	if context.Bounds.X+context.Bounds.W > budget.Bounds.X || context.Bounds.Y != budget.Bounds.Y {
		t.Fatalf("the rings overlap: %+v and %+v", context.Bounds, budget.Bounds)
	}
	for _, hint := range []string{"Plain ring", "Other plain ring"} {
		if _, drawn := tt.Access(hint); !drawn {
			t.Errorf("no ring named %q", hint)
		}
	}
	tt.ClickAt(budget.Bounds.X+budget.Bounds.W/2, budget.Bounds.Y+budget.Bounds.H/2)
	tt.Frame()
	tt.Frame()
	if open[first] || !open[second] {
		t.Fatalf("pressing the second ring opened %v", open)
	}
	if tt.HasText("panel of Context 25%") || !tt.HasText("panel of Budget 60%") {
		t.Fatalf("the panel shown is not the second ring's: %q", tt.Texts())
	}
	if context, _ = tt.AccessByRole(ui.RoleButton, "Context 25%"); context.Expanded {
		t.Error("the first ring reads as expanded")
	}
}

// A docked assistant's composer draws no context bar: the location row the
// page's composer shows under the input is not there.
func TestDockedHasNoContextBar(t *testing.T) {
	for _, docked := range []bool{false, true} {
		tt := tray("en", func() *composer.Context {
			return &composer.Context{Activity: composer.Activity{Connected: true}, Docked: docked}
		})
		if _, drawn := tt.Access("Chats"); drawn == docked {
			t.Errorf("docked=%v: location shown=%v: %q", docked, drawn, tt.Texts())
		}
	}
}
