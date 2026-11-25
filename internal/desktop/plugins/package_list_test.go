package plugins

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestSelectableCardList(t *testing.T) {
	native := NewNativeBinding(t.Context())
	components, err := NativeComponents(native)
	if err != nil {
		t.Fatal(err)
	}
	owner := NewHost(HostBinding{Context: t.Context(), Native: &native})
	t.Cleanup(func() { _ = owner.Close() })
	modules, err := owner.Modules()
	if err != nil {
		t.Fatal(err)
	}
	runtime, err := NewRuntime(RuntimeBindings{Modules: modules, Components: components})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = runtime.Close() })
	bundle, scope := runtimeBundle(`
import {View,div} from "gpui-kit";
import {IconButton,SelectableRow} from "sailry/ui";
export default class Fixture extends View {
  render(){return div().v_flex().w(384).gap_2().child(IconButton.new('new-item',{
    icon:'plus',label:'New item',show_label:true,full_width:true,variant:'primary',size:'medium'}))
    .children([{id:'a',name:'First item',address:'first.example:22'},
      {id:'b',name:'Second item',address:'second.example:2222'}].map(item=>
      SelectableRow.new('open-'+item.id,{variant:'card',label:item.name,selected:false})
        .child(div().v_flex().w_full().gap_0p5().child(item.name).child(item.address))));}
}`)
	mounted, err := runtime.Mount(bundle, scope)
	if err != nil {
		t.Fatal(err)
	}
	var renderErr error
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Dark)
		ui.Column(view).Fill().Children(func() { _, renderErr = mounted.Render(view) })
	}, 1024, 800)
	for range 3 {
		tester.Frame()
	}
	if renderErr != nil {
		t.Fatal(renderErr)
	}
	find := func(text string) ui.Rect {
		t.Helper()
		box, exists := tester.Find(text)
		if !exists || box.W <= 0 || box.H <= 0 || box.X < 0 || box.Y < 0 || box.X+box.W > 1024 || box.Y+box.H > 800 {
			t.Fatalf("%q is not laid out inside the page: %v, %q", text, box, tester.Texts())
		}
		return box
	}
	button := find("New item")
	first, address := find("First item"), find("first.example:22")
	second := find("Second item")
	find("second.example:2222")
	if first.Y < button.Y+button.H || second.Y < first.Y+first.H || first.X != button.X || first.W != button.W {
		t.Fatalf("cards are not listed under the button: button %v, rows %v %v", button, first, second)
	}
	if address.X < first.X+13 || address.Y < first.Y+13 || address.Y+address.H > first.Y+first.H-13+0.5 {
		t.Fatalf("the detail %v is not inside its card %v", address, first)
	}
	if err := tester.Click("Second item"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	event, err := native.next(t.Context(), "control")
	if err != nil || !strings.Contains(string(event), `"id":"open-b"`) {
		t.Fatalf("a card click reported %s, %v", event, err)
	}
}
