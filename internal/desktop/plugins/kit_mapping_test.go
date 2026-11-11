package plugins

import (
	"encoding/json"
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// testKitMapping asserts that package components are the application's own
// Kit controls: a step of TestElementMapping.
func testKitMapping(t *testing.T) {
	t.Run("page, select and empty list", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		name := ""
		var failure error
		keep := func(_ ui.Element, err error) {
			if err != nil && failure == nil {
				failure = err
			}
		}
		element := func(id uint64, export, props string) NativeElement {
			return NativeElement{ID: id, Module: "sailry/ui", Export: export, Props: json.RawMessage(props)}
		}
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			keep(native.renderLayout(view, element(1, "SettingsPage", `["page",{"title":"Fixture tasks","description":"Run fixture tasks"}]`), func() {
				ui.Row(view.Key("toolbar")).FillWidth().Gap(12).Justify(ui.SpaceBetween).Children(func() {
					ui.Column(view).Width(192).Children(func() {
						keep(native.renderControl(view, element(2, "SelectField",
							`["filter",{"label":"Project filter","placeholder":"All projects","selected":null,"items":[{"id":"a","label":"Alpha"},{"id":"b","label":"Beta","disabled":true}]}]`), nil, nil))
					})
					ui.TextInput(view.Key("input"), &name).Label("Name").Width(120)
				})
				keep(native.renderLayout(view, element(3, "CardList", `["list",{"loaded":true,"empty_icon":"calendar","empty_label":"No fixture tasks"}]`), func() {}, nil))
			}, nil))
		}, 1000, 600)
		tester.Frame()
		tester.Frame()
		if failure != nil {
			t.Fatal(failure)
		}
		title, _ := tester.Find("Fixture tasks")
		description, _ := tester.Find("Run fixture tasks")
		field, exists := tester.Access("Project filter")
		input, _ := tester.Access("Name")
		// The application's page: an 800 column centered in 1000, padded 24.
		if !exists || title.X != 124 || description.X != 124 || field.Bounds.X != 124 {
			t.Fatalf("left edges differ: title %v, description %v, select %v", title, description, field.Bounds)
		}
		// The Kit select: a control frame as high as a text input, filling
		// the width its package gives it, showing the placeholder.
		if field.Bounds.H != input.Bounds.H || field.Bounds.W != 192 || !tester.HasText("All projects") {
			t.Fatalf("select frame %v, input %v, text %q", field.Bounds, input.Bounds, tester.Texts())
		}
		empty, exists := tester.Find("No fixture tasks")
		if !exists || math.Abs(float64(empty.X+empty.W/2-500)) > 0.5 {
			t.Fatalf("empty state is not centered in the content column: %v", empty)
		}
		// Kit's Result: a 48 round tile above the title, in a card as wide
		// as the column.
		if empty.Y-field.Bounds.Y-field.Bounds.H < 24+40+48+12 {
			t.Fatalf("empty state lacks its tile and padding: title %v, select %v", empty, field.Bounds)
		}
		tester.ClickAt(field.Bounds.X+10, field.Bounds.Y+5)
		tester.Frame()
		tester.Advance(200 * time.Millisecond)
		menu, exists := tester.AccessByRole(ui.RoleMenu, "")
		if !exists || menu.Bounds.W != field.Bounds.W || !tester.HasText("Alpha") || !tester.HasText("Beta") {
			t.Fatalf("select did not open the Kit menu at its width: %v, %v", menu.Bounds, field.Bounds)
		}
		if err := tester.Click("Alpha"); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		event, err := native.next(t.Context(), "control")
		if err != nil || string(event) != `{"id":"filter","value":"a"}` {
			t.Fatalf("select choice was not reported: %s %v", event, err)
		}
	})
	t.Run("empty state panel", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		var failure error
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			ui.Column(view).Fill().Children(func() {
				_, failure = native.renderLayout(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "EmptyState",
					Props: json.RawMessage(`["empty",{"icon":"database","label":"No fixture connections"}]`)}, func() {}, nil)
			})
		}, 800, 600)
		tester.Frame()
		if failure != nil {
			t.Fatal(failure)
		}
		label, exists := tester.Find("No fixture connections")
		if !exists || math.Abs(float64(label.X+label.W/2-400)) > 0.5 || label.Y < 300 || label.Y > 360 {
			t.Fatalf("panel empty state is not centered in its area: %v", label)
		}
	})
	t.Run("scroll viewport percentages and button labels", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class Example extends View {render(){
  return div().relative().size_full().child(div().absolute().inset(0).overflow_x_scroll().overflow_y_scroll()
    .child(div().w_full().min_h("100%").v_flex().items_center()
      .child(div().v_flex().items_center().gap_2().flex_shrink(0).my_auto()
        .child(new Button("outline").outline().label("Outlined"))
        .child(new Button("plain").label("Plain"))
        .child(new Button("danger").danger().label("Dangerous")))));
}}`, RuntimeBindings{})
		var renderErr error
		tester := ui.NewTester(func(context *ui.Context) {
			theme.Apply(context, "", theme.Light)
			ui.Column(context).Fill().Children(func() { _, renderErr = mounted.Render(context) })
			ui.Row(context).Absolute().Top(0).Left(0).Children(func() {
				kit.Secondary(context, "Outlined").Label("Kit outlined")
				kit.Danger(context, "Dangerous").Label("Kit dangerous")
			})
		}, 480, 300)
		for range 4 {
			tester.Frame()
		}
		if renderErr != nil {
			t.Fatal(renderErr)
		}
		// A package's buttons are Kit's, each with its label once.
		for label, kitLabel := range map[string]string{"Outlined": "Kit outlined", "Dangerous": "Kit dangerous"} {
			button, _ := tester.Access(label)
			reference, exists := tester.Access(kitLabel)
			if !exists || button.Bounds.W != reference.Bounds.W || button.Bounds.H != reference.Bounds.H {
				t.Fatalf("%s button %v is not Kit's %v", label, button.Bounds, reference.Bounds)
			}
		}
		// The content centers in the scroll viewport on both axes, as the
		// reference resolves a percentage against it.
		plain, _ := tester.Access("Plain")
		if math.Abs(float64(plain.Bounds.X+plain.Bounds.W/2-240)) > 1 || math.Abs(float64(plain.Bounds.Y+plain.Bounds.H/2-150)) > 1 {
			t.Fatalf("scrolled content is not centered: %v", plain.Bounds)
		}
	})
}
