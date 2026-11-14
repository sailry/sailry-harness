package plugins

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestDetailsReadRuntimeTypography(t *testing.T) {
	_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class extends View {
  init(){this.open=false;}
  render(cx){
    const body=div().v_flex().child(new Button("details").label("Details").on_click((event,cx)=>{this.open=true;cx.notify();}));
    if(this.open){
      const typography=cx.theme().typography;
      body.child(div().font_family(typography.mono).child("09:00"))
        .child(div().child(typography.mono+"/"+typography.md.size+"/"+typography.mono_md.size));
    }
    return body;
  }
}`, RuntimeBindings{})
	mode := theme.Light
	var renderErr error
	tester := ui.NewTester(func(context *ui.Context) {
		theme.Apply(context, "", mode)
		_, renderErr = mounted.Render(context)
	}, 420, 260)
	tester.Frame()
	if renderErr != nil {
		t.Fatal(renderErr)
	}
	if err := tester.Click("Details"); err != nil {
		t.Fatal(err)
	}
	for _, current := range []theme.Mode{theme.Light, theme.Dark} {
		mode = current
		tester.Frame()
		if renderErr != nil || !tester.HasText("09:00") || !tester.HasText("monospace/16/13") {
			t.Fatalf("details lost runtime typography in mode %v: %v", mode, renderErr)
		}
	}
}
