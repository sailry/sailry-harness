package plugins

import (
	"encoding/json"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestWorkspaceLayout asserts a Workspace's resource column stands before
// its content, its details go to the Shell's right sidebar instead of the
// page, and a Workspace that names neither keeps its children in a column.
func TestWorkspaceLayout(t *testing.T) {
	label := func(id uint64, text string) *NativeElement {
		props, _ := json.Marshal([]any{map[string]any{"text": text}})
		return &NativeElement{ID: id, Module: "test", Export: "Label", Props: props}
	}
	draw := func(t *testing.T, native *NativeBinding, props string, sidebar *func(*ui.Context) error) *ui.Tester {
		workspace := &NativeElement{ID: 1, Module: "sailry/ui", Export: "Workspace", Props: json.RawMessage(props),
			Children: []*NativeElement{label(2, "Content"), label(3, "Details"), label(4, "Navigation")}}
		workspace.RenderChild = func(view *ui.Context, element *NativeElement) (ui.Element, error) {
			var text []map[string]string
			if err := json.Unmarshal(element.Props, &text); err != nil {
				return ui.Element{}, err
			}
			return ui.Text(view.Key(element.ID), text[0]["text"]), nil
		}
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			ui.Row(view).Fill().Children(func() {
				ui.Column(view.Key("page")).Grow(1).Basis(0).Children(func() {
					if _, err := native.renderLayout(view, *workspace, func() {
						for _, child := range workspace.Children {
							_, _ = workspace.RenderChild(view, child)
						}
					}, nil); err != nil {
						t.Fatal(err)
					}
				})
				if sidebar != nil && *sidebar != nil {
					ui.Column(view.Key("sidebar")).Width(320).Children(func() {
						if err := (*sidebar)(view); err != nil {
							t.Fatal(err)
						}
					})
				}
			})
		}, 1000, 600)
		tester.Frame()
		tester.Frame()
		return tester
	}
	find := func(t *testing.T, tester *ui.Tester, text string) ui.Rect {
		t.Helper()
		bounds, ok := tester.Find(text)
		if !ok {
			t.Fatalf("missing %q", text)
		}
		return bounds
	}
	const resource = `["workspace",{"has_details":true,"navigation":"resource","min_navigation_width":200}]`
	t.Run("resource column and sidebar details", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		var sidebar func(*ui.Context) error
		native.Details = func(render func(*ui.Context) error) { sidebar = render }
		tester := draw(t, &native, resource, &sidebar)
		navigation, content, details := find(t, tester, "Navigation"), find(t, tester, "Content"), find(t, tester, "Details")
		if navigation.X != 0 || content.X < workspaceNavigation || content.Y != navigation.Y {
			t.Fatalf("navigation=%+v content=%+v, want the resource column before the content", navigation, content)
		}
		if details.X < 680 || details.Y != content.Y {
			t.Fatalf("details=%+v, want them in the sidebar", details)
		}
	})
	t.Run("details stay in the page without a sidebar", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		tester := draw(t, &native, resource, nil)
		content, details := find(t, tester, "Content"), find(t, tester, "Details")
		if details.X != content.X || details.Y <= content.Y {
			t.Fatalf("content=%+v details=%+v, want the details under the content", content, details)
		}
	})
	t.Run("no details and no navigation", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		supplied := true
		native.Details = func(render func(*ui.Context) error) { supplied = render != nil }
		tester := draw(t, &native, `["workspace",{"has_details":false,"navigation":"none"}]`, nil)
		content, details, navigation := find(t, tester, "Content"), find(t, tester, "Details"), find(t, tester, "Navigation")
		if supplied || details.Y <= content.Y || navigation.Y <= details.Y || navigation.X != content.X {
			t.Fatalf("supplied=%v content=%+v details=%+v navigation=%+v, want one column and no sidebar details", supplied, content, details, navigation)
		}
	})
}
