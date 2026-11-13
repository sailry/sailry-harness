package plugins

import (
	"fmt"
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestTransitionMapping(t *testing.T) {
	for _, test := range []struct {
		easing string
		width  float32
	}{
		{"linear", 45},
		{"ease-in", 21.5625},
		{"ease-out", 77.8125},
		{"ease-in-out", 26.25},
	} {
		t.Run(test.easing, func(t *testing.T) {
			_, mounted := runtimeFixture(t, fmt.Sprintf(`
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class extends View {
  init(){this.width=20;}
  render(){return div().v_flex().items_start()
    .child(div().id("tile").w(this.width).h(20).accessibility_label("Animated tile")
      .transition("width",{duration:200,delay:100,easing:%q}))
    .child(new Button("move").label("Move").on_click((_,cx)=>{this.width=120;cx.notify();}));}
}`, test.easing), RuntimeBindings{})
			var renderErr error
			window := ui.NewTester(func(view *ui.Context) {
				theme.Apply(view, "", theme.Light)
				_, renderErr = mounted.Render(view)
			}, 300, 150)
			checkWidth := func(want float32) {
				t.Helper()
				tile, exists := window.Access("Animated tile")
				if renderErr != nil || !exists || math.Abs(float64(tile.Bounds.W-want)) > 0.05 {
					t.Fatalf("animated width = %v, want %.4f (visible %t): %v", tile.Bounds, want, exists, renderErr)
				}
			}
			window.Frame()
			checkWidth(20)
			if err := window.Click("Move"); err != nil {
				t.Fatal(err)
			}
			window.Frame()
			window.Advance(75 * time.Millisecond)
			window.Frame()
			checkWidth(20)
			window.Advance(75 * time.Millisecond)
			window.Frame()
			checkWidth(test.width)
			window.Advance(200 * time.Millisecond)
			window.Frame()
			checkWidth(120)
		})
	}
}
