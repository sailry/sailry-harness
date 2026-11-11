package theme

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// TestNeutral covers the theme a list is built under to mark its chosen rows
// as the app's other neutral selections: the Selected fill under the normal
// foreground in the accent's place, everything else and the palette as the
// frame's.
func TestNeutral(t *testing.T) {
	for _, dark := range []bool{false, true} {
		name := map[bool]string{false: "light", true: "dark"}[dark]
		t.Run(name, func(t *testing.T) {
			var widget, neutral, nested *ui.Theme
			var palette, inside *Palette
			tester := ui.NewTesterWith(func(c *ui.Context) {
				c.SetTheme(Widget(BuiltinID, dark))
				widget, neutral, palette = c.Theme(), Neutral(c), Of(c)
				ui.Themed(c, neutral).Children(func() { inside, nested = Of(c), Neutral(c) })
			}, 200, 100, ui.TesterOptions{})
			tester.Frame()
			if neutral == widget || palette == nil || inside != palette {
				t.Fatalf("the neutral theme is the frame's own, or has another palette: %p %p, %p %p", neutral, widget, inside, palette)
			}
			if nested != neutral {
				t.Fatal("the neutral theme is not its own neutral theme")
			}
			fill := palette.Selected()
			if neutral.Accent != fill || neutral.AccentHover != fill || neutral.AccentPressed != fill || neutral.InactiveAccent != fill ||
				neutral.AccentText != palette.Foreground {
				t.Fatalf("the neutral theme marks with %+v under %+v, want %+v under %+v", neutral.Accent, neutral.AccentText, fill, palette.Foreground)
			}
			if fill == palette.Primary || widget.Accent != palette.Primary {
				t.Fatalf("the frame's theme lost its accent: %+v", widget.Accent)
			}
			// Nothing else differs.
			restored := *neutral
			restored.Accent, restored.AccentHover, restored.AccentPressed = widget.Accent, widget.AccentHover, widget.AccentPressed
			restored.InactiveAccent, restored.AccentText = widget.InactiveAccent, widget.AccentText
			if restored != *widget {
				t.Fatal("the neutral theme changes more than the accent")
			}
		})
	}
}
