package plugins

import (
	"encoding/json"
	"fmt"
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// testSizedButtons asserts a package's own size, padding and alignment win
// over a button's defaults: a step of testKitMapping. The tree is the
// statistics context panel's history, a flex row of 16 slots with gap 4
// whose measured slots hold a tooltip around an unstyled Button.
func testSizedButtons(t *testing.T) {
	native := NewNativeBinding(t.Context())
	components, err := NativeComponents(native)
	if err != nil {
		t.Fatal(err)
	}
	_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-base";
import {Button as Styled} from "gpui-component";
import {Tooltip} from "sailry/ui";
export default class Example extends View {render(){
  const slots=Array.from({length:16},(_,index)=>{
    const slot=div().accessibility_label("slot "+index).flex_1().min_w(0).h(28);
    if(index>=3) return slot.v_flex().justify_end().child(div().w_full().h(3).rounded_t(1).bg("#808080"));
    const bar=(index===2?new Styled("bar-"+index):Button.new("bar-"+index)).w_full().h(28).p(0).items_end()
      .selected(index===1).accessibility_label("button "+index)
      .child(div().w_full().flex_shrink_0().h(12+index*8).rounded_t(2).bg("#ff0000"));
    return slot.child(Tooltip.new("tip-"+index,{text:"Step "+index}).child(bar));
  });
  return div().v_flex().w(320).p(10)
    .child(div().accessibility_label("row").h_flex().w_full().h(28).items_end().gap(4).children(slots))
    .child(Button.new("fold").w_full().min_h(22).py(2).accessibility_label("fold").selected(true).child(div().child("Details")))
    .child(Tooltip.new("row-tip",{text:"Note"}).child(div().accessibility_label("tipped").h_flex().w_full().py(4).child("Row")))
    .child(div().h_flex().child(new Styled("padded").py(2).label("Padded")).child(new Styled("small").size("xsmall").pt(1).label("Small")));
}}`, RuntimeBindings{Components: components})
	var renderErr error
	tester := ui.NewTester(func(context *ui.Context) {
		theme.Apply(context, "", theme.Light)
		ui.Column(context).Fill().Children(func() { _, renderErr = mounted.Render(context) })
		ui.Row(context).Absolute().Top(150).Left(0).Children(func() {
			kit.Secondary(context, "Padded").Label("Kit padded")
			kit.Secondary(context, "Small").Label("Kit small").Height(20).PaddingX(8)
		})
	}, 480, 200)
	for range 3 {
		tester.Frame()
	}
	if renderErr != nil {
		t.Fatal(renderErr)
	}
	bounds := func(label string) ui.Rect {
		node, exists := tester.Access(label)
		if !exists {
			t.Fatalf("%s is not laid out", label)
		}
		return node.Bounds
	}
	near := func(a, b float32) bool { return math.Abs(float64(a-b)) <= 0.5 }
	same := func(a, b ui.Rect) bool { return near(a.X, b.X) && near(a.Y, b.Y) && near(a.W, b.W) && near(a.H, b.H) }
	frame := tester.Image()
	red := func(x, y int) bool {
		pixel := frame.RGBAAt(x, y)
		return pixel.R > 200 && pixel.G < 60 && pixel.B < 60
	}
	row := bounds("row")
	width := (row.W - 15*4) / 16
	var previous ui.Rect
	for index := range 16 {
		slot := bounds(fmt.Sprintf("slot %d", index))
		// Sixteen equal slots four apart, whatever they hold.
		if want := (ui.Rect{X: row.X + float32(index)*(width+4), Y: row.Y, W: width, H: 28}); !same(slot, want) {
			t.Fatalf("slot %d is %v, want %v", index, slot, want)
		}
		if index > 0 && slot.X < previous.X+previous.W+3.5 {
			t.Fatalf("slot %d at %v overlaps slot %d at %v", index, slot, index-1, previous)
		}
		previous = slot
		if index >= 3 {
			continue
		}
		// The button is its slot: the package's size and padding replace
		// the host's, also on Kit's own button (the third).
		if button := bounds(fmt.Sprintf("button %d", index)); !same(button, slot) {
			t.Fatalf("button %d is %v, its slot %v", index, button, slot)
		}
		if index == 2 {
			continue
		}
		// An unstyled button's bar spans the slot at its bottom: no
		// border or minimum size of the host's is left.
		height := float32(12 + index*8)
		left, right, top, bottom := int(slot.X), int(slot.X+slot.W)-1, int(slot.Y+slot.H-height), int(slot.Y+slot.H)-1
		for _, at := range [][2]int{{left, bottom}, {right, bottom}, {left + 3, top + 1}, {right - 3, top + 1}} {
			if !red(at[0], at[1]) {
				t.Fatalf("bar %d does not fill its slot %v to %v high: nothing at %v", index, slot, height, at)
			}
		}
		for _, at := range [][2]int{{left - 1, bottom}, {right + 1, bottom}, {left + 3, top - 1}} {
			if red(at[0], at[1]) {
				t.Fatalf("bar %d leaves its slot %v or its height %v at %v", index, slot, height, at)
			}
		}
	}
	// An unstyled button draws nothing of its own, selected or not: above
	// its bar the first two slots show the page.
	ground := frame.RGBAAt(int(row.X+row.W)+4, int(row.Y)+2)
	for index := range 2 {
		slot := bounds(fmt.Sprintf("slot %d", index))
		for _, x := range []float32{slot.X, slot.X + slot.W/2, slot.X + slot.W - 1} {
			if pixel := frame.RGBAAt(int(x), int(slot.Y)+2); pixel != ground {
				t.Fatalf("button %d draws %v over the page's %v at %v", index, pixel, ground, x)
			}
		}
	}
	// A full-width unstyled button with only a minimum height is as high as
	// its content needs, and no wider than its parent.
	fold := bounds("fold")
	if !near(fold.X, row.X) || !near(fold.W, row.W) || fold.H < 22 || fold.H > 26 {
		t.Fatalf("the fold trigger is %v under a row %v", fold, row)
	}
	if pixel := frame.RGBAAt(int(fold.X+fold.W)-3, int(fold.Y+fold.H/2)); pixel != ground {
		t.Fatalf("the selected fold trigger draws %v over the page's %v", pixel, ground)
	}
	// A tooltip's trigger adds no box: the row it holds is as wide as the
	// column and as high as its content.
	if tipped := bounds("tipped"); !near(tipped.X, row.X) || !near(tipped.W, row.W) || tipped.H < 20 || tipped.H > 30 || tipped.Y < fold.Y+fold.H-0.5 {
		t.Fatalf("the row under a tooltip is %v, the column %v", tipped, row)
	}
	// A style naming one edge of Kit's button keeps its others.
	for label, kitLabel := range map[string]string{"Padded": "Kit padded", "Small": "Kit small"} {
		if button, reference := bounds(label), bounds(kitLabel); !near(button.W, reference.W) || !near(button.H, reference.H) {
			t.Fatalf("%s button %v lost the padding of Kit's %v", label, button, reference)
		}
	}
}

// testChartTokens asserts a package's chart_1 to chart_8 are the theme's
// chart colors in order, in both modes: a step of testKitMapping.
func testChartTokens(t *testing.T) {
	for _, dark := range []bool{false, true} {
		palette := theme.Look("", dark)
		var snapshot struct {
			Colors map[string]string `json:"colors"`
		}
		if err := json.Unmarshal(ThemeSnapshot(palette), &snapshot); err != nil {
			t.Fatal(err)
		}
		seen := map[string]int{}
		for index, color := range palette.Chart {
			name := fmt.Sprintf("chart_%d", index+1)
			want := fmt.Sprintf("#%02x%02x%02x%02x", color.R, color.G, color.B, color.A)
			if snapshot.Colors[name] != want {
				t.Errorf("dark %v: %s = %q, want the theme's %s", dark, name, snapshot.Colors[name], want)
			}
			if other, taken := seen[want]; taken {
				t.Errorf("dark %v: %s repeats chart_%d", dark, name, other)
			}
			seen[want] = index + 1
		}
		if _, extra := snapshot.Colors["chart_9"]; extra || len(palette.Chart) != 8 {
			t.Errorf("dark %v: the snapshot holds more chart colors than the contract's eight", dark)
		}
	}
}
