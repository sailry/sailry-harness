package plugins

import (
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestRuntimeTagInRow asserts a small tag in a row lower than a control
// stays as high as its text, centered beside the row's name, and that a
// secondary tag is neutral rather than Kit's primary tint.
func TestRuntimeTagInRow(t *testing.T) {
	_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Tag} from "gpui-component";
export default class Example extends View {render(){return div().h(22).h_flex().items_center().gap_2()
  .child(div().text_sm().child("Name"))
  .child(new Tag().variant("secondary").size("small").child("Acting"))
  .child(new Tag().size("small").child("Plain"));}}`, RuntimeBindings{})
	var renderErr error
	tester := ui.NewTester(func(context *ui.Context) {
		theme.Apply(context, "", theme.Light)
		_, renderErr = mounted.Render(context)
	}, 480, 120)
	tester.Frame()
	tester.Frame()
	if renderErr != nil {
		t.Fatal(renderErr)
	}
	name, named := tester.Find("Name")
	tag, tagged := tester.Find("Acting")
	plain, shown := tester.Find("Plain")
	if !named || !tagged || !shown {
		t.Fatalf("missing text: %q", tester.Texts())
	}
	if tag.H > 22 || math.Abs(float64(tag.Y+tag.H/2-(name.Y+name.H/2))) > 1 {
		t.Fatalf("the tag is not centered beside the name: tag %v, name %v", tag, name)
	}
	at := func(r ui.Rect) [3]uint8 {
		px := tester.Image().RGBAAt(int(r.X+2), int(r.Y+r.H/2))
		return [3]uint8{px.R, px.G, px.B}
	}
	if secondary, primary := at(tag), at(plain); secondary == primary || int(secondary[2]) > int(secondary[0])+8 {
		t.Fatalf("a secondary tag keeps the primary tint: %v, %v", secondary, primary)
	}
}
