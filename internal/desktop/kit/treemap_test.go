package kit

import (
	"math"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestSquarify asserts the layout fills its rectangle in proportion to the
// values, without overlap, and keeps tiles near square.
func TestSquarify(t *testing.T) {
	values := []float64{6, 6, 4, 3, 2, 2, 1, 0}
	r := ui.Rect{X: 10, Y: 20, W: 600, H: 400}
	got := squarify(values, r)
	total := 24.0
	for i, v := range values {
		tile := got[i]
		if v == 0 {
			if tile.W != 0 || tile.H != 0 {
				t.Fatalf("a zero value got %+v", tile)
			}
			continue
		}
		want := v / total * float64(r.W*r.H)
		if area := float64(tile.W * tile.H); math.Abs(area-want) > want*0.001 {
			t.Errorf("value %v has area %v, want %v", v, area, want)
		}
		if tile.X < r.X-0.01 || tile.Y < r.Y-0.01 || tile.X+tile.W > r.X+r.W+0.01 || tile.Y+tile.H > r.Y+r.H+0.01 {
			t.Errorf("tile %d %+v leaves %+v", i, tile, r)
		}
		if ratio := max(tile.W/tile.H, tile.H/tile.W); ratio > 4 {
			t.Errorf("tile %d is %v to 1", i, ratio)
		}
		for j := range i {
			other := got[j]
			if other.W == 0 {
				continue
			}
			w := min(tile.X+tile.W, other.X+other.W) - max(tile.X, other.X)
			h := min(tile.Y+tile.H, other.Y+other.H) - max(tile.Y, other.Y)
			if w > 0.01 && h > 0.01 {
				t.Errorf("tiles %d and %d overlap", i, j)
			}
		}
	}
}

// TestTreemap asserts the map names a large tile on it, a small one only
// in its accessible label, and each group with its total in the legend.
func TestTreemap(t *testing.T) {
	groups := []TreemapGroup{
		{ID: "heap", Label: "Heap", Detail: "96 MB", Items: []TreemapItem{{ID: "objects", Label: "Objects", Detail: "90 MB", Value: 90}, {ID: "tiny", Label: "Tiny", Detail: "1 KB", Value: 0.8}}},
		{ID: "stacks", Label: "Stacks", Detail: "8 MB", Items: []TreemapItem{{ID: "goroutines", Label: "Goroutines", Detail: "8 MB", Value: 8}}},
		{ID: "none", Label: "Empty", Items: []TreemapItem{{ID: "zero", Label: "Zero", Value: 0}}},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		ui.Column(c).Fill().Padding(16).Children(func() { Treemap(c, "memory", groups, TreemapOptions{}) })
	}, 800, 500)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	for range 3 {
		tester.Frame()
	}
	for _, text := range []string{"Objects", "90 MB · Heap", "Heap", "96 MB", "Stacks"} {
		if !tester.HasText(text) {
			t.Fatalf("missing %q: %q", text, tester.Texts())
		}
	}
	// Texts holds the tiles' labels too: a hidden title is an exact entry.
	if texts := tester.Texts(); slices.Contains(texts, "Tiny") || slices.Contains(texts, "Empty") || tester.HasText("Zero") {
		t.Fatalf("a tiny tile or an empty group shows text: %q", tester.Texts())
	}
	if _, ok := tester.Access("Tiny · 1 KB · Heap"); !ok {
		t.Error("a small tile has no accessible label")
	}
}

// TestTreemapSpacing checks the rendered boxes of a narrow category beside
// the heap, including its short final tiles, rather than just the layout's
// rectangles. Padding must not enlarge a small tile into the next one.
func TestTreemapSpacing(t *testing.T) {
	groups := []TreemapGroup{
		{ID: "heap", Label: "Heap", Items: []TreemapItem{{ID: "objects", Label: "Objects", Value: 96}}},
		{ID: "runtime", Label: "Runtime", Items: []TreemapItem{
			{ID: "collector", Label: "Collector", Value: 3.6},
			{ID: "allocator", Label: "Allocator", Value: 0.2},
			{ID: "profiling", Label: "Profiling", Value: 0.2},
		}},
	}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		Treemap(c, "memory", groups, TreemapOptions{Height: 194})
	}, 394, 300)
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	for range 3 {
		tester.Frame()
	}
	bounds := map[string]ui.Rect{}
	for _, tile := range []struct {
		label string
		want  ui.Rect
	}{
		{"Objects · Heap", ui.Rect{X: 0, Y: 0, W: 378, H: 194}},
		{"Collector · Runtime", ui.Rect{X: 384, Y: 0, W: 10, H: 174}},
		{"Allocator · Runtime", ui.Rect{X: 384, Y: 180, W: 10, H: 4}},
		{"Profiling · Runtime", ui.Rect{X: 384, Y: 190, W: 10, H: 4}},
	} {
		got, ok := tester.Access(tile.label)
		if !ok {
			t.Fatalf("missing tile %q", tile.label)
		}
		bounds[tile.label] = got.Bounds
		for _, delta := range []float32{got.Bounds.X - tile.want.X, got.Bounds.Y - tile.want.Y, got.Bounds.W - tile.want.W, got.Bounds.H - tile.want.H} {
			if math.Abs(float64(delta)) > 0.01 {
				t.Errorf("tile %q rendered at %+v, want %+v", tile.label, got.Bounds, tile.want)
				break
			}
		}
	}
	heap, collector := bounds["Objects · Heap"], bounds["Collector · Runtime"]
	allocator, profiling := bounds["Allocator · Runtime"], bounds["Profiling · Runtime"]
	for _, gap := range []float32{
		collector.X - heap.X - heap.W,
		allocator.Y - collector.Y - collector.H,
		profiling.Y - allocator.Y - allocator.H,
	} {
		if math.Abs(float64(gap-treemapGap)) > 0.01 {
			t.Errorf("rendered tile gap is %v, want %v", gap, treemapGap)
		}
	}
}

// TestTreemapTinyBorders keeps even a sub-two-DIP tile at its assigned
// size; a fixed one-DIP border would widen or heighten it into its gap.
func TestTreemapTinyBorders(t *testing.T) {
	for _, test := range []struct {
		name          string
		first, second ui.Rect
	}{
		{"narrow", ui.Rect{X: 10, Y: 10, W: 1, H: 30}, ui.Rect{X: 17, Y: 10, W: 30, H: 30}},
		{"short", ui.Rect{X: 10, Y: 10, W: 30, H: 0.5}, ui.Rect{X: 10, Y: 16.5, W: 30, H: 30}},
	} {
		t.Run(test.name, func(t *testing.T) {
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Dark)
				ui.Box(c).Fill().Children(func() {
					group := TreemapGroup{ID: "runtime", Label: "Runtime"}
					treemapTile(c, group, TreemapItem{ID: "first", Label: "First"}, test.first, theme.Of(c).Series(0))
					treemapTile(c, group, TreemapItem{ID: "second", Label: "Second"}, test.second, theme.Of(c).Series(0))
				})
			}, 120, 80)
			tester.SetPreferences(ui.Preferences{ReduceMotion: true})
			for range 3 {
				tester.Frame()
			}
			first, ok := tester.Access("First · Runtime")
			if !ok || first.Bounds != test.first {
				t.Fatalf("first tile rendered at %+v (%v), want %+v", first.Bounds, ok, test.first)
			}
			second, ok := tester.Access("Second · Runtime")
			if !ok || second.Bounds != test.second {
				t.Fatalf("second tile rendered at %+v (%v), want %+v", second.Bounds, ok, test.second)
			}
			gap := second.Bounds.X - first.Bounds.X - first.Bounds.W
			if test.name == "short" {
				gap = second.Bounds.Y - first.Bounds.Y - first.Bounds.H
			}
			if gap != treemapGap {
				t.Errorf("rendered tile gap is %v, want %v", gap, treemapGap)
			}
		})
	}
}
