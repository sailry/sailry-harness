package plugins

import (
	"encoding/json"
	"math"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// testKitMapping asserts that package components are the application's own
// Kit controls: a step of TestElementMapping.
func testKitMapping(t *testing.T) {
	t.Run("a chosen table row is neutral", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		var failure error
		tester := ui.NewTesterWith(func(view *ui.Context) {
			theme.Apply(view, theme.BuiltinID, theme.Light) // the application's theme, which the neutral one is made of
			ui.Column(view).Fill().Children(func() {
				if _, err := native.renderTable(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "DataTable",
					Props: json.RawMessage(`["table",{"columns":["Name","Size"],"rows":[["alpha","1"],["beta","2"]]}]`)}, nil, nil); err != nil && failure == nil {
					failure = err
				}
			})
		}, 600, 300, ui.TesterOptions{})
		tester.Frame()
		tester.Frame()
		if failure != nil {
			t.Fatal(failure)
		}
		chosen, shown := tester.Find("alpha")
		other, also := tester.Find("beta")
		if !shown || !also {
			t.Fatalf("the table lists no rows: %q", tester.Texts())
		}
		tester.ClickAt(chosen.X+chosen.W/2, chosen.Y+chosen.H/2)
		tester.Move(2, 2)
		tester.Frame()
		tester.Frame()
		// A point of each row past its cell's text, where only the row's
		// fill is drawn.
		at := func(row ui.Rect) [3]int {
			pixel := tester.Image().RGBAAt(int(row.X+row.W+40), int(row.Y+row.H/2))
			return [3]int{int(pixel.R), int(pixel.G), int(pixel.B)}
		}
		fill, ground := at(chosen), at(other)
		if fill == ground {
			t.Fatalf("the chosen row is not filled: %v", fill)
		}
		d := [3]int{fill[0] - ground[0], fill[1] - ground[1], fill[2] - ground[2]}
		if max(d[0], d[1], d[2])-min(d[0], d[1], d[2]) > 3 {
			t.Fatalf("the chosen row is filled %v over %v, want a neutral gray", fill, ground)
		}
	})
	t.Run("table rows are as high as declared under a filled header", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		var failure error
		tester := ui.NewTesterWith(func(view *ui.Context) {
			theme.Apply(view, theme.BuiltinID, theme.Light)
			ui.Column(view).Fill().Padding(20).Children(func() {
				if _, err := native.renderTable(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "DataTable",
					Props: json.RawMessage(`["table",{"columns":["Name","Size"],"rows":[["alpha","1"],["beta","2"],["gamma","3"]],"row_height":36}]`)}, nil, nil); err != nil && failure == nil {
					failure = err
				}
			})
		}, 600, 300, ui.TesterOptions{})
		tester.Frame()
		tester.Frame()
		if failure != nil {
			t.Fatal(failure)
		}
		title, headed := tester.Find("Name")
		first, shown := tester.Find("alpha")
		second, also := tester.Find("beta")
		if !headed || !shown || !also {
			t.Fatalf("the table lists no rows: %q", tester.Texts())
		}
		// The declared height is the row's, cell padding included.
		if pitch := second.Y - first.Y; pitch < 35.5 || pitch > 36.5 {
			t.Fatalf("rows are %v apart, want the declared 36", pitch)
		}
		// Past the cell's text only the table's own fills are drawn.
		at := func(y float32) [3]uint8 {
			pixel := tester.Image().RGBAAt(int(first.X+first.W+40), int(y))
			return [3]uint8{pixel.R, pixel.G, pixel.B}
		}
		middle := first.Y + first.H/2
		ground := at(middle)
		if at(title.Y+title.H/2) == ground {
			t.Fatalf("the header is not filled: %v", ground)
		}
		separated := false
		for y := middle + 12; y <= middle+24; y++ {
			separated = separated || at(y) != ground
		}
		if !separated {
			t.Fatal("no separator between the first two rows")
		}
	})
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
	t.Run("list rows", testListRows)
	t.Run("package-sized buttons", testSizedButtons)
	t.Run("chart tokens", testChartTokens)
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

// testListRows asserts a package's settings and card rows keep the
// application's row metrics, a row menu named by an icon is an icon button,
// a sortable group reorders by drag and by keyboard, and an icon name no
// set knows leaves the row without an icon: a step of testKitMapping.
func testListRows(t *testing.T) {
	native := NewNativeBinding(t.Context())
	var failure error
	keep := func(_ ui.Element, err error) {
		if err != nil && failure == nil {
			failure = err
		}
	}
	node := func(id uint64, export, props string, children ...*NativeElement) *NativeElement {
		return &NativeElement{ID: id, Module: "sailry/ui", Export: export, Props: json.RawMessage(props), Children: children}
	}
	bounds := map[uint64]ui.Rect{}
	var render func(*ui.Context, *NativeElement) (ui.Element, error)
	render = func(view *ui.Context, element *NativeElement) (ui.Element, error) {
		view = view.Key(element.ID)
		element.RenderChild = render
		children := func() {
			for _, child := range element.Children {
				keep(render(view, child))
			}
		}
		draw := native.renderLayout
		if element.Export == "Menu" || element.Export == "Toggle" {
			draw = native.renderControl
		}
		result, err := draw(view, *element, children, nil)
		if err == nil {
			bounds[element.ID] = result.LayoutBounds()
		}
		return result, err
	}
	menu := func(id uint64, name string) *NativeElement {
		return node(id, "Menu", `["`+name+`",{"label":"More","icon":"ellipsis","items":[{"id":"edit","label":"Edit"}]}]`)
	}
	services := []*NativeElement{
		node(11, "SettingsRow", `["row-exa",{"label":"Exa","description":"Anonymous quota","icon":"ailogo:exa"}]`, menu(21, "more-exa")),
		node(12, "SettingsRow", `["row-brave",{"label":"Brave","description":"API key required","icon":"ailogo:brave"}]`, menu(22, "more-brave")),
		node(13, "SettingsRow", `["row-model",{"label":"Model search"}]`, menu(23, "more-model")),
	}
	sortable := node(10, "SettingsGroup", `["services",{"title":"Services","sortable":true,"sort_label":"Reorder"}]`, services...)
	plain := node(30, "SettingsGroup", `["plain",{"title":"Plain"}]`,
		node(31, "SettingsRow", `["row-plugin",{"label":"Plugin row","description":"Plugin description"}]`,
			node(41, "Toggle", `["plugin-switch",{"label":"Plugin switch","checked":true}]`)),
		node(32, "SettingsRow", `["row-icon",{"label":"Icon row","description":"Icon description","icon":"play"}]`),
		node(33, "SettingsRow", `["row-unknown",{"label":"Unknown row","icon":"ailogo:none"}]`))
	card := node(50, "CardRow", `["card",{}]`, node(51, "CardSummary", `["summary",{"title":"Card row","subtitle":"Card description"}]`))
	on := true
	var core, single ui.Rect
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		ui.Column(view).Fill().Padding(24).Gap(16).Children(func() {
			kit.Card(view, func() {
				core = kit.Row(view, "core", "Core row", "Core description", func() {
					ui.Switch(view.Key("switch"), &on).Label("Core switch")
				}).LayoutBounds()
				single = kit.Row(view, "single", "Single row", "", nil).LayoutBounds()
			})
			// The package orders its rows as the last reorder event said.
			sortable.Children = services
			keep(render(view, sortable))
			keep(render(view, plain))
			keep(render(view, card))
		})
	}, 800, 900)
	settle := func() {
		tester.Frame()
		tester.Advance(time.Second)
		tester.Frame()
		tester.Frame()
		if failure != nil {
			t.Fatal(failure)
		}
	}
	settle()
	find := func(text string) ui.Rect {
		t.Helper()
		box, exists := tester.Find(text)
		if !exists {
			t.Fatalf("%q is not shown: %q", text, tester.Texts())
		}
		return box
	}
	// One vertical rhythm: the application's row, the package's settings
	// row and its card row are as high, their titles and descriptions as
	// large and as far apart, and padded alike. A card row's own 1 DIP
	// border lies outside those metrics.
	rhythm := func(title, description string, row ui.Rect) [5]float32 {
		first, second := find(title), find(description)
		return [5]float32{row.H, first.H, second.H, second.Y - first.Y - first.H, first.Y - row.Y}
	}
	want := rhythm("Core row", "Core description", core)
	if want[0] != kit.RowPad*2+want[1]+kit.TitleGap+want[2] || want[3] != kit.TitleGap || want[4] != kit.RowPad || single.H != kit.RowMinHeight {
		t.Fatalf("core row metrics %v, single row %v", want, single)
	}
	for name, got := range map[string][5]float32{
		"settings row":   rhythm("Plugin row", "Plugin description", bounds[31]),
		"sortable row":   rhythm("Exa", "Anonymous quota", bounds[11]),
		"card row":       rhythm("Card row", "Card description", ui.Rect{X: bounds[50].X + 1, Y: bounds[50].Y + 1, W: bounds[50].W - 2, H: bounds[50].H - 2}),
		"icon row":       rhythm("Icon row", "Icon description", bounds[32]),
		"core row again": want,
	} {
		if got != want {
			t.Errorf("%s metrics %v, the core row's %v (height, title, description, gap, top padding)", name, got, want)
		}
	}
	if bounds[13].H != kit.RowMinHeight || bounds[33].H != kit.RowMinHeight {
		t.Errorf("title-only rows are %v and %v high, want %v", bounds[13].H, bounds[33].H, float32(kit.RowMinHeight))
	}
	// An unknown icon name is no icon and no error. Rows beside one with an
	// icon keep their text behind the slot; a group without icons has none.
	if left := find("Core row").X; find("Plugin row").X != left+kit.RowIcon+kit.RowIconGap || find("Icon row").X != find("Plugin row").X || find("Unknown row").X != find("Plugin row").X {
		t.Errorf("icon group text: core %v, plugin %v, icon %v, unknown %v", left, find("Plugin row").X, find("Icon row").X, find("Unknown row").X)
	}
	if find("Exa").X != find("Brave").X || find("Exa").X != find("Model search").X {
		t.Errorf("rows without icons are not aligned: %v %v %v", find("Exa"), find("Brave"), find("Model search"))
	}
	// The overflow menu is an icon button: its name is kept and no text
	// repeats it.
	named := func(name string) (found []ui.AccessInfo) {
		for _, info := range tester.AccessAll() {
			if info.Name == name {
				found = append(found, info)
			}
		}
		return
	}
	shown := 0
	for _, text := range tester.Texts() {
		if text == "More" {
			shown++
		}
	}
	more := named("More")
	if len(more) != 3 || shown != 3 || more[0].Role != ui.RoleMenuButton || more[0].Bounds.W != 28 || more[0].Bounds.H != 28 {
		t.Fatalf("overflow menus: %d named More, %d texts, first %v", len(more), shown, more)
	}
	tester.ClickAt(more[0].Bounds.X+14, more[0].Bounds.Y+14)
	tester.Frame()
	tester.Advance(200 * time.Millisecond)
	tester.Frame()
	if err := tester.Click("Edit"); err != nil {
		t.Fatal(err)
	}
	settle()
	if event, err := native.next(t.Context(), "menu"); err != nil || string(event) != `{"id":"edit","menu":"more-exa"}` {
		t.Fatalf("menu choice: %s %v", event, err)
	}
	// No arrow buttons: the handle moves a row.
	handles := named("Reorder")
	if len(handles) != 3 || len(named("Move up"))+len(named("Move down")) != 0 {
		t.Fatalf("%d handles", len(handles))
	}
	// reordered applies the event as the package does.
	reordered := func(want string) {
		t.Helper()
		event, err := native.next(t.Context(), "control")
		if err != nil || string(event) != want {
			t.Fatalf("reorder event %s %v, want %s", event, err, want)
		}
		var body struct{ Value struct{ Order []string } }
		if err := json.Unmarshal(event, &body); err != nil {
			t.Fatal(err)
		}
		next := make([]*NativeElement, len(body.Value.Order))
		for index, name := range body.Value.Order {
			next[index] = services[slices.IndexFunc(services, func(row *NativeElement) bool {
				identity, _, _ := nativeProps(*row)
				return identity == name
			})]
		}
		services = next
		settle()
	}
	// Dragging the first row's handle under the last row drops it there.
	grip, last := handles[0].Bounds, bounds[13]
	x, y := grip.X+grip.W/2, grip.Y+grip.H/2
	tester.Press(x, y)
	for step := 1; step <= 8; step++ {
		tester.Move(x, y+(last.Y+last.H/2-y)*float32(step)/8)
		tester.Frame()
	}
	tester.Release(x, last.Y+last.H/2)
	tester.Frame()
	reordered(`{"id":"services","value":{"from":0,"order":["row-brave","row-model","row-exa"],"to":2}}`)
	if first, second, third := find("Brave"), find("Model search"), find("Exa"); first.Y >= second.Y || second.Y >= third.Y {
		t.Fatalf("page order after the drag: Brave %v, Model search %v, Exa %v", first, second, third)
	}
	// The keyboard: Space lifts the focused handle's row, the arrows move
	// it, Space drops it.
	handles = named("Reorder")
	tester.ClickAt(handles[2].Bounds.X+handles[2].Bounds.W/2, handles[2].Bounds.Y+handles[2].Bounds.H/2)
	for _, key := range []ui.Key{ui.KeySpace, ui.KeyUp, ui.KeyUp, ui.KeySpace} {
		tester.Key(0, key)
		tester.Frame()
	}
	reordered(`{"id":"services","value":{"from":2,"order":["row-exa","row-brave","row-model"],"to":0}}`)
	if first, second, third := find("Exa"), find("Brave"), find("Model search"); first.Y >= second.Y || second.Y >= third.Y {
		t.Fatalf("page order after the keys: Exa %v, Brave %v, Model search %v", first, second, third)
	}
	// A locked group keeps its handles and takes no move.
	sortable.Props = json.RawMessage(`["services",{"title":"Services","sortable":true,"sort_label":"Reorder","sort_disabled":true}]`)
	settle()
	handles = named("Reorder")
	if len(handles) != 3 || !handles[0].Disabled {
		t.Fatalf("locked handles: %v", handles)
	}
	// A sortable group names its handles.
	sortable.Props = json.RawMessage(`["services",{"title":"Services","sortable":true}]`)
	tester.Frame()
	if failure == nil {
		t.Fatal("a sortable group without a sort label was drawn")
	}
}
