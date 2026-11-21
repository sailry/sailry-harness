package conversation

import (
	"image"
	"image/color"
	"image/draw"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestHTMLPreviewCardFitsScreenshotWithPadding(t *testing.T) {
	screenshot := image.NewRGBA(image.Rect(0, 0, 300, 240))
	draw.Draw(screenshot, screenshot.Bounds(), image.White, image.Point{}, draw.Src)
	corners := [...]color.RGBA{
		{R: 255, A: 255}, {G: 255, A: 255}, {B: 255, A: 255}, {R: 255, B: 255, A: 255},
	}
	for i, shade := range corners {
		corner := image.Pt(i%2*276, i/2*216)
		draw.Draw(screenshot, image.Rectangle{Min: corner, Max: corner.Add(image.Pt(24, 24))}, image.NewUniform(shade), image.Point{}, draw.Src)
	}
	file := File{ID: "html", Path: "preview.html", Mime: "text/html", Document: true, Interactive: true,
		Image: &Image{Source: ui.NewBitmap(screenshot)},
	}

	for _, test := range []struct {
		name  string
		width int
	}{
		{"wide card", 480},
		{"narrow card", 180},
	} {
		t.Run(test.name, func(t *testing.T) {
			state, actions := New(), &previewCardActions{}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				state.fileCard(c, "preview", file, actions)
			}, test.width, 300)
			tester.SetLocale("en")
			tester.Frame()
			var preview ui.Rect
			for _, node := range tester.AccessAll() {
				if node.Role == ui.RoleButton && node.Name == i18n.In("en", "artifact_open") && node.Bounds.H > preview.H {
					preview = node.Bounds
				}
			}
			if preview.W <= 0 || preview.H <= 0 {
				t.Fatal("missing preview button")
			}
			frame := tester.Image()
			slot := image.Rect(int(preview.X), int(preview.Y), int(preview.X+preview.W), int(preview.Y+preview.H))
			seen := make(map[color.RGBA]bool, len(corners))
			var painted image.Rectangle
			for y := slot.Min.Y; y < slot.Max.Y; y++ {
				for x := slot.Min.X; x < slot.Max.X; x++ {
					shade := frame.RGBAAt(x, y)
					for _, corner := range corners {
						if shade == corner {
							seen[corner] = true
							painted = painted.Union(image.Rect(x, y, x+1, y+1))
						}
					}
				}
			}
			if len(seen) != len(corners) {
				t.Fatalf("preview displays %d of %d screenshot corners", len(seen), len(corners))
			}
			if !painted.In(slot.Inset(10)) {
				t.Errorf("image %v must have padding inside preview %v", painted, slot)
			}
			tester.ClickAt(preview.X+preview.W/2, preview.Y+preview.H/2)
			if actions.opened != file.ID {
				t.Errorf("preview opened %q, want %q", actions.opened, file.ID)
			}
		})
	}
}

type previewCardActions struct {
	still
	opened string
}

func (a *previewCardActions) OpenFile(f File) { a.opened = f.ID }
