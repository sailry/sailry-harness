package plugins

import (
	"encoding/json"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestSettingsRowBrandIcons(t *testing.T) {
	cases := []struct {
		name, icon string
		colored    bool
	}{
		{"Exa", "ailogo:exa-color", true}, {"Tavily", "ailogo:tavily-color", true}, {"Brave", "ailogo:brave-color", true},
		{"SearXNG", "ailogo:searxng-color", true}, {"Bocha", "ailogo:bocha-color", true},
		{"Volcengine", "ailogo:volcengine-color", true}, {"DeepSeek", "ailogo:deepseek-color", true},
		{"Explicit mono", "ailogo:alibaba-brand", false},
		{"TinyFish fallback", "globe", false}, {"Model search fallback", "bot", false},
	}
	for _, mode := range []theme.Mode{theme.Light, theme.Dark} {
		name := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
		t.Run(name, func(t *testing.T) {
			native := NewNativeBinding(t.Context())
			var failure error
			var reference ui.Rect
			rows := make([]ui.Rect, len(cases))
			clicked := ""
			tester := ui.NewTesterWith(func(view *ui.Context) {
				theme.Apply(view, "", mode)
				ui.Column(view).Fill().Padding(24).Background(view.Theme().Background).Children(func() {
					kit.Row(view, "reference", "Reference", "", func() {
						ui.Button(view, "Configure reference")
					}).DrawOver(func(_ *ui.Painter, bounds ui.Rect) { reference = bounds })
					for index, item := range cases {
						props, _ := json.Marshal([]any{item.name, map[string]any{"label": item.name, "icon": item.icon}})
						row, err := native.renderLayout(view.Key(index), NativeElement{ID: uint64(index + 1), Module: "sailry/ui", Export: "SettingsRow", Props: props,
							Children: []*NativeElement{{ID: uint64(index + 100)}}}, func() {
							ui.Button(view, "Configure "+item.name).OnClick(func() { clicked = item.name })
						}, nil)
						if err != nil && failure == nil {
							failure = err
						}
						row.DrawOver(func(_ *ui.Painter, bounds ui.Rect) { rows[index] = bounds })
					}
				})
			}, 600, 760, ui.TesterOptions{Dark: mode == theme.Dark})
			for range 3 {
				tester.Frame()
			}
			if failure != nil {
				t.Fatal(failure)
			}
			picture := tester.Image()
			ground := picture.RGBAAt(1, 1)
			// A native button may exceed the minimum row height on a system
			// font. Brand icons must retain the same geometry as the core row.
			if reference.H < kit.RowMinHeight {
				t.Fatalf("core row is below its minimum height: %v", reference)
			}
			for index, item := range cases {
				label, exists := tester.Find(item.name)
				if !exists || label.X != 24+kit.RowIcon+kit.RowIconGap || rows[index].H != reference.H {
					t.Fatalf("%s changed row geometry: row %v, label %v", item.name, rows[index], label)
				}
				inked, colored := 0, 0
				for y := int(rows[index].Y); y < int(rows[index].Y+rows[index].H); y++ {
					for x := 24; x < 24+kit.RowIcon; x++ {
						pixel := picture.RGBAAt(x, y)
						if pixel != ground {
							inked++
						}
						if max(pixel.R, pixel.G, pixel.B)-min(pixel.R, pixel.G, pixel.B) > 20 {
							colored++
						}
					}
				}
				if inked < 10 || item.colored && colored < 10 || !item.colored && colored != 0 {
					t.Errorf("%s draws %d pixels of ink and %d in its own colors", item.name, inked, colored)
				}
			}
			if err := tester.Click("Configure Exa"); err != nil || clicked != "Exa" {
				t.Fatalf("row action changed: clicked %q, %v", clicked, err)
			}
		})
	}
}
