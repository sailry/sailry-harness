package plugins

import (
	"context"
	"encoding/json"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestKeyboardMapping(t *testing.T) {
	t.Run("native field bubbles Escape and keeps ordinary input", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		components, err := NativeComponents(native)
		if err != nil {
			t.Fatal(err)
		}
		forms := HostModule{Name: "sailry/forms"}
		for _, name := range []string{"createText", "focusText"} {
			forms.Exports = append(forms.Exports, hostSync(name, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
				return native.formOperation(ctx, name, arguments)
			}))
		}
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {TextField,createText,focusText} from "sailry/forms";
export default class extends View {
  init(){this.field=createText("draft",{label:"Path editor"});focusText(this.field);this.cancelled=0;this.parent=0;}
  render(){return div().v_flex().on_key_down((event,cx)=>{if(event.key==="escape"){this.parent++;cx.notify();}})
    .child(div().on_key_down((event,cx)=>{if(event.key==="escape"){this.cancelled++;cx.stop_propagation();cx.notify();}}).child(TextField.new(this.field)))
    .child("Cancelled "+this.cancelled+" Parent "+this.parent);}
}`, RuntimeBindings{Modules: []HostModule{forms}, Components: components})
		var renderErr error
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			_, renderErr = mounted.Render(view)
		}, 420, 200)
		for range 3 {
			tester.Frame()
		}
		if renderErr != nil || !tester.Focused("Path editor") {
			t.Fatalf("native field has no focus: %v", renderErr)
		}
		tester.Key(0, ui.KeyEscape)
		tester.Frame()
		if renderErr != nil || !tester.HasText("Cancelled 1 Parent 0") {
			t.Fatalf("Escape did not stop at the inner script handler: %v, %q", renderErr, tester.Texts())
		}
		tester.Key(ui.Cmd, ui.KeyA)
		tester.Type("kept")
		tester.Frame()
		field, exists := tester.Access("Path editor")
		if !exists || field.Value != "kept" {
			t.Fatalf("unhandled keys replaced the field's native input: %+v", field)
		}
	})
	t.Run("tracked focus receives history keys and propagates unhandled keys", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class extends View {
  init(_props,cx){this.history=cx.focus_handle();this.row=0;this.copies=0;this.opens=0;this.parent=0;}
  render(){return div().v_flex().on_key_down((event,cx)=>{this.parent++;cx.notify();})
    .child(div().id("history").accessibility_label("History keys").track_focus(this.history).on_key_down((event,cx)=>{
      if(event.key==="up")this.row++;
      else if(event.key==="down")this.row--;
      else if(event.key==="c"&&(event.modifiers.platform||event.modifiers.control))this.copies++;
      else if(event.key==="enter")this.opens++;
      else return;
      cx.stop_propagation();cx.notify();
    }).child(new Button("choose").label("Select history").on_click((_,cx)=>{this.history.focus();cx.notify();})))
    .child("Row "+this.row+" Copies "+this.copies+" Opens "+this.opens+" Parent "+this.parent);}
}`, RuntimeBindings{})
		var renderErr error
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			_, renderErr = mounted.Render(view)
		}, 500, 220)
		tester.Frame()
		if err := tester.Click("Select history"); err != nil {
			t.Fatal(err)
		}
		for range 3 {
			tester.Frame()
		}
		if renderErr != nil || !tester.Focused("History keys") {
			t.Fatalf("track_focus did not focus the native keyboard target: %v", renderErr)
		}
		for _, stroke := range []struct {
			mods ui.Modifiers
			key  ui.Key
		}{{0, ui.KeyUp}, {0, ui.KeyUp}, {0, ui.KeyDown}, {ui.Cmd, ui.KeyC}, {ui.Ctrl, ui.KeyC}, {0, ui.KeyEnter}, {0, ui.KeyLeft}} {
			tester.Key(stroke.mods, stroke.key)
			tester.Frame()
		}
		if renderErr != nil || !tester.HasText("Row 1 Copies 2 Opens 1 Parent 1") {
			t.Fatalf("history keyboard behavior changed: %v, %q", renderErr, tester.Texts())
		}
	})
	t.Run("retired suspended and closed input never calls scripts", func(t *testing.T) {
		calls := []string{}
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {record} from "fixture";
export default class extends View {
  init(_props,cx){this.target=cx.focus_handle();this.target.focus();this.count=0;}
  render(){return div().track_focus(this.target).accessibility_label("Lifecycle keys").on_key_down((event,cx)=>{record(event.key);this.count++;cx.stop_propagation();cx.notify();}).child("Keys "+this.count);}
}`, RuntimeBindings{Modules: []HostModule{{Name: "fixture", Exports: []HostExport{hostSync("record", func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
			value, err := hostValue[string](arguments, 0)
			if err != nil {
				return nil, err
			}
			calls = append(calls, value)
			return json.RawMessage(`null`), nil
		})}}}})
		var renderErr error
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			if !mounted.closed {
				_, renderErr = mounted.Render(view)
			}
		}, 420, 160)
		for range 3 {
			tester.Frame()
		}
		stale := mounted.last.keyInput
		tester.Key(0, ui.KeyUp)
		tester.Frame()
		if renderErr != nil || !slices.Equal(calls, []string{"up"}) {
			t.Fatalf("initial key was not dispatched: %v, %q", renderErr, calls)
		}
		// The shell can paint a preceding snapshot in its navigation slot.
		// One newer generation therefore keeps its input callback alive.
		stale(ui.InputEvent{Kind: ui.InputKeyDown, Key: ui.KeyDown})
		tester.Frame()
		if renderErr != nil || !slices.Equal(calls, []string{"up", "down"}) {
			t.Fatalf("preceding snapshot input was lost: %v, %q", renderErr, calls)
		}
		// Two newer snapshots release that callback, as ordinary dispatch
		// does, before a late native handler can try to use it again.
		stale(ui.InputEvent{Kind: ui.InputKeyDown, Key: ui.KeyDown})
		mounted.Suspend()
		tester.Key(0, ui.KeyDown)
		tester.Frame()
		if err := mounted.Close(); err != nil {
			t.Fatal(err)
		}
		stale(ui.InputEvent{Kind: ui.InputKeyDown, Key: ui.KeyEnter})
		tester.Key(0, ui.KeyEnter)
		tester.Frame()
		if renderErr != nil || !slices.Equal(calls, []string{"up", "down"}) {
			t.Fatalf("retired input called the script: %v, %q", renderErr, calls)
		}
	})
}
