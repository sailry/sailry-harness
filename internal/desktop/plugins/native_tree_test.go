package plugins

import (
	"encoding/json"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func testTreeRows(t *testing.T) {
	for _, variant := range []string{"resource", "branch"} {
		t.Run(variant, func(t *testing.T) {
			native := NewNativeBinding(t.Context())
			items := []nativeTreeItem{
				{ID: "folder", Label: "Folder", Children: []nativeTreeItem{{ID: "child", Label: "Child"}}},
				{ID: "file", Label: "File"},
			}
			current := ""
			var selected []string
			tester := ui.NewTester(func(view *ui.Context) {
				theme.Apply(view, "", theme.Light)
				encoded, err := json.Marshal(items)
				if err != nil {
					t.Fatal(err)
				}
				props, err := json.Marshal([]any{"tree", map[string]any{
					"items": string(encoded), "variant": variant, "current": current, "selected": selected,
				}})
				if err != nil {
					t.Fatal(err)
				}
				ui.Column(view).Fill().Children(func() {
					_, err = native.renderTree(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "ResourceTree", Props: props}, nil, nil)
					if err != nil {
						t.Fatal(err)
					}
				})
			}, 400, 300)
			frame := func() {
				tester.Frame()
				tester.Frame()
			}
			row := func(label string) ui.AccessInfo {
				t.Helper()
				info, exists := tester.AccessByRole(ui.RoleTreeItem, label)
				if !exists {
					t.Fatalf("tree item %q is not shown", label)
				}
				return info
			}
			event := func(id, kind string) {
				t.Helper()
				select {
				case body := <-native.queue("tree", 128).events:
					var got struct{ Tree, ID, Kind string }
					if err := json.Unmarshal(body, &got); err != nil || got.Tree != "tree" || got.ID != id || got.Kind != kind {
						t.Fatalf("tree event %s (%v), want %s %s", body, err, id, kind)
					}
				default:
					t.Fatalf("missing %s event for %s", kind, id)
				}
			}
			click := func(label, id string) {
				t.Helper()
				current = id
				selected = []string{id}
				info := row(label)
				tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
				frame()
				event(id, "select")
			}
			frame()
			if row("Folder").Expanded || tester.HasText("Child") || row("File").Level != 1 {
				t.Fatal("initial tree hierarchy is incorrect")
			}
			click("Folder", "folder")
			event("folder", "expand")
			if !row("Folder").Expanded || row("Child").Level != 2 {
				t.Fatal("expanding a folder did not show its child")
			}
			click("Child", "child")
			event("child", "open")
			// The package can reorder its roots without moving the focused
			// child to another item. Expansion remains package-declared.
			items[0].Expanded = true
			items[0], items[1] = items[1], items[0]
			frame()
			if child := row("Child"); !child.Focused || !child.Selected || child.Level != 2 || row("File").Focused {
				t.Fatalf("reordered tree lost its child identity: %+v", child)
			}
			selected = []string{"child", "file"}
			frame()
			if !row("Child").Selected || !row("File").Selected || row("Folder").Selected {
				t.Fatal("multiple selected rows were replaced by the branch cursor")
			}
			click("Folder", "folder")
			event("folder", "collapse")
			if row("Folder").Expanded || tester.HasText("Child") {
				t.Fatal("collapsing a folder left its child visible")
			}
		})
	}
}
