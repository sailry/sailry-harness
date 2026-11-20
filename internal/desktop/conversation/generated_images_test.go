package conversation

import (
	"image"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestGeneratedImagePreviewOnlyInAnswer(t *testing.T) {
	for _, test := range []struct {
		name, tool string
		answer     bool
		want       int
	}{
		{"completed generation", "generate_image", true, 1},
		{"generation still processing", "generate_image", false, 0},
		{"screenshot details", "take_screenshot", true, 2},
	} {
		t.Run(test.name, func(t *testing.T) {
			picture := Image{ID: "picture", Name: "result.png", Reference: "call/0", Width: 80, Height: 60,
				Source: ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 80, 60)))}
			call := Call{ID: "call", Name: test.tool, Label: "Inspect execution", Output: &Output{
				Text: "Execution parameters and status", Images: []Image{picture},
			}}
			state := New()
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				ui.Column(c).Fill().Children(func() {
					state.row(c.Key("process"), call, false)
					if test.answer {
						state.thumbnails(c.Key("answer"), "answer-images", []Image{picture}, false)
					}
				})
			}, 900, 700)
			tester.SetLocale("en")
			tester.Frame()
			if err := tester.Click(call.Label); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			tester.Frame()
			if !tester.HasText(call.Output.Text) {
				t.Fatal("execution details disappeared with the generated preview")
			}
			count := 0
			for _, item := range tester.AccessAll() {
				if item.Role == ui.RoleButton && item.Name == picture.Name {
					count++
				}
			}
			if count != test.want {
				t.Fatalf("visible pictures = %d, want %d", count, test.want)
			}
			if len(call.Output.Images) != 1 || call.Output.Images[0].Reference != picture.Reference || call.Output.Images[0].Source != picture.Source {
				t.Fatal("hiding the execution preview changed its canonical view references")
			}
		})
	}
}
