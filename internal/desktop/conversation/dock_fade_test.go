package conversation

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestDockFade(t *testing.T) {
	width, height := float32(320), float32(280)
	const reserve = float32(100)
	tester := ui.NewTester(func(c *ui.Context) {
		ui.Box(c).Fill().Background(ui.RGB(255, 255, 255)).Children(func() {
			list := ui.Box(c).Width(width).Height(height).Background(ui.RGB(255, 0, 0))
			fadeUnder(list, list.Bounds(), reserve)
		})
	}, 440, 360)
	tester.Frame()
	for _, size := range [][2]float32{{320, 280}, {240, 320}, {400, 300}} {
		width, height = size[0], size[1]
		tester.Frame()
		image := tester.Image()
		end := height - reserve
		for _, point := range []struct {
			x, y  float32
			green int
		}{
			{20, end - dockFade - 4, 0},
			{20, end - dockFade/2, 128},
			{20, end + 4, 255},
			{width - scrollStrip - 2, height - 4, 255},
			{width - scrollStrip + 2, height - 4, 0},
		} {
			pixel := image.RGBAAt(int(point.x), int(point.y))
			difference := int(pixel.G) - point.green
			if pixel.R != 255 || pixel.A != 255 || difference < -8 || difference > 8 || pixel.B != pixel.G {
				t.Errorf("at size %.0fx%.0f, point (%.0f,%.0f): %v, want red over white with green %d", width, height, point.x, point.y, pixel, point.green)
			}
		}
	}
	t.Run("fractional placement preserves sharp content", func(t *testing.T) {
		for _, scale := range []float32{1, 1.5, 2} {
			show := func(masked bool) *ui.Tester {
				tester := ui.NewTester(func(c *ui.Context) {
					ui.Box(c).Fill().Background(ui.RGB(255, 255, 255)).Children(func() {
						list := ui.Row(c).Absolute().Left(10.3).Top(7.2).Width(320.35).Height(280.6)
						list.Children(func() {
							for i := range 321 {
								color := ui.RGB(255, 0, 0)
								if i%2 == 0 {
									color = ui.RGB(0, 0, 255)
								}
								ui.Box(c).Width(1).Shrink(0).FillHeight().Background(color)
							}
						})
						if masked {
							fadeUnder(list, list.Bounds(), reserve)
						}
					})
				}, 440, 360)
				tester.SetScale(scale)
				tester.Frame()
				return tester
			}
			plain, faded := show(false).Image(), show(true).Image()
			for _, area := range [][4]float32{{20, 30, 300, 150}, {318, 220, 328, 270}} {
				for y := int(area[1] * scale); y < int(area[3]*scale); y++ {
					for x := int(area[0] * scale); x < int(area[2]*scale); x++ {
						if got, want := faded.RGBAAt(x, y), plain.RGBAAt(x, y); got != want {
							t.Fatalf("scale %g, (%d,%d): copied content %v, original %v", scale, x, y, got, want)
						}
					}
				}
			}
		}
	})
}
