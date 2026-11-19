package plugins

import (
	"encoding/json"
	"fmt"
	"image"
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestHeatmapDrawsSquareCells(t *testing.T) {
	for _, sample := range []struct {
		name           string
		width, columns int
	}{
		{name: "short range on desktop", width: 900, columns: 5},
		{name: "short range in narrow space", width: 80, columns: 5},
		{name: "year on desktop", width: 900, columns: 53},
	} {
		t.Run(sample.name, func(t *testing.T) {
			binding := NewNativeBinding(t.Context())
			var renderErr error
			tester := ui.NewTester(func(view *ui.Context) {
				theme.Apply(view, "", theme.Dark)
				ui.Column(view).Fill().Children(func() {
					renderErr = renderTestHeatmap(&binding, view, map[string]json.RawMessage{
						"rows":    json.RawMessage("7"),
						"columns": json.RawMessage(fmt.Sprint(sample.columns)),
						"cells":   json.RawMessage(`[{"column":0,"row":0,"intensity":1}]`),
					})
				})
			}, sample.width, 230)
			tester.Frame()
			if renderErr != nil {
				t.Fatal(renderErr)
			}
			frame := tester.Image()
			fill := frame.RGBAAt(5, 5)
			var bounds image.Rectangle
			for y := range frame.Bounds().Dy() {
				for x := range frame.Bounds().Dx() {
					if frame.RGBAAt(x, y) == fill {
						bounds = bounds.Union(image.Rect(x, y, x+1, y+1))
					}
				}
			}
			slot := min(float64(sample.width)/float64(sample.columns), 24)
			if expected := slot * 0.8; math.Abs(float64(bounds.Dx())-expected) > 1 || math.Abs(float64(bounds.Dy())-expected) > 1 {
				t.Fatalf("visible cell bounds %v, want about %.1f square", bounds, expected)
			}
		})
	}
}

func TestHeatmapFillsMeasuredWidth(t *testing.T) {
	for _, width := range []int{250, 900} {
		t.Run(fmt.Sprint(width), func(t *testing.T) {
			const rows = 7
			columns := (width + 17) / 18
			cells := fmt.Sprintf(`[{"column":0,"row":0,"intensity":0},{"column":%d,"row":6,"intensity":1}]`, columns-1)
			binding := NewNativeBinding(t.Context())
			var renderErr error
			tester := ui.NewTester(func(view *ui.Context) {
				theme.Apply(view, "", theme.Dark)
				ui.Column(view).Fill().Children(func() {
					renderErr = renderTestHeatmap(&binding, view, map[string]json.RawMessage{
						"rows":    json.RawMessage(fmt.Sprint(rows)),
						"columns": json.RawMessage(fmt.Sprint(columns)),
						"cells":   json.RawMessage(cells),
					})
				})
			}, width, 230)
			tester.Frame()
			if renderErr != nil {
				t.Fatal(renderErr)
			}
			frame := tester.Image()
			pitch := float64(width) / float64(columns)
			first := frame.RGBAAt(int(0.4*pitch), int(0.4*pitch))
			last := frame.RGBAAt(int(float64(width)-0.6*pitch), int((rows-0.6)*pitch))
			background := frame.RGBAAt(width-1, 0)
			if first == background || last == background || first == last {
				t.Fatalf("gray first and active last cells do not span the plot: %v, %v, background %v", first, last, background)
			}
		})
	}
}

func renderTestHeatmap(binding *NativeBinding, view *ui.Context, props map[string]json.RawMessage) error {
	source, err := parseHeatmap(props)
	if err == nil {
		binding.renderHeatmap(view, source)
	}
	return err
}
