package plugins

import (
	"encoding/json"
	"errors"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestWorkspaceLayout asserts that resource navigation and details are
// supplied to the Shell and never drawn twice inside the page.
func TestWorkspaceLayout(t *testing.T) {
	label := func(id uint64, text string) *NativeElement {
		props, _ := json.Marshal([]any{map[string]any{"text": text}})
		return &NativeElement{ID: id, Module: "test", Export: "Label", Props: props}
	}
	draw := func(t *testing.T, native *NativeBinding, props string, sidebar, navigation *func(*ui.Context) error) *ui.Tester {
		calls := map[uint64]int{}
		workspace := &NativeElement{ID: 1, Module: "sailry/ui", Export: "Workspace", Props: json.RawMessage(props),
			Children: []*NativeElement{label(2, "Content"), label(3, "Details"), label(4, "Navigation")}}
		workspace.RenderChild = func(view *ui.Context, element *NativeElement) (ui.Element, error) {
			calls[element.ID]++
			if calls[element.ID] != 1 {
				t.Fatalf("element %d rendered twice in one frame", element.ID)
			}
			var text []map[string]string
			if err := json.Unmarshal(element.Props, &text); err != nil {
				return ui.Element{}, err
			}
			return ui.Text(view.Key(element.ID), text[0]["text"]), nil
		}
		tester := ui.NewTester(func(view *ui.Context) {
			clear(calls)
			theme.Apply(view, "", theme.Light)
			ui.Row(view).Fill().Children(func() {
				if navigation != nil && *navigation != nil {
					ui.Column(view.Key("navigation")).Width(240).Children(func() {
						if err := (*navigation)(view); err != nil {
							t.Fatal(err)
						}
					})
				}
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
	t.Run("navigation and details leave the page", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		var sidebar, navigation func(*ui.Context) error
		native.Details = func(render func(*ui.Context) error) { sidebar = render }
		native.Navigation = func(render func(*ui.Context) error) { navigation = render }
		tester := draw(t, &native, resource, &sidebar, &navigation)
		navigationBounds, content, details := find(t, tester, "Navigation"), find(t, tester, "Content"), find(t, tester, "Details")
		if navigationBounds.X != 0 || content.X != 240 || content.Y != navigationBounds.Y {
			t.Fatalf("navigation=%+v content=%+v, want navigation only in the Shell slot", navigationBounds, content)
		}
		if details.X < 680 || details.Y != content.Y {
			t.Fatalf("details=%+v, want them in the sidebar", details)
		}
	})
	t.Run("details stay in the page without a sidebar", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		tester := draw(t, &native, resource, nil, nil)
		content, details := find(t, tester, "Content"), find(t, tester, "Details")
		if details.X != content.X || details.Y <= content.Y {
			t.Fatalf("content=%+v details=%+v, want the details under the content", content, details)
		}
		if tester.HasText("Navigation") {
			t.Fatal("resource navigation was drawn inside the page without a Shell slot")
		}
	})
	t.Run("no details and no navigation", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		supplied := true
		native.Details = func(render func(*ui.Context) error) { supplied = render != nil }
		tester := draw(t, &native, `["workspace",{"has_details":false,"navigation":"none"}]`, nil, nil)
		content, details, navigation := find(t, tester, "Content"), find(t, tester, "Details"), find(t, tester, "Navigation")
		if supplied || details.Y <= content.Y || navigation.Y <= details.Y || navigation.X != content.X {
			t.Fatalf("supplied=%v content=%+v details=%+v navigation=%+v, want one column and no sidebar details", supplied, content, details, navigation)
		}
	})
	t.Run("changed navigation invalidates once", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		invalidated := 0
		native.Invalidate = func() { invalidated++ }
		draw(t, &native, resource, nil, nil)
		if invalidated != 1 {
			t.Fatalf("stable navigation requested %d frames, want one", invalidated)
		}
		layout := native.WorkspaceLayout()
		if layout.ID != "workspace" || layout.MinNavigationWidth != 200 || layout.MinDetailsWidth != 260 || layout.DefaultDetailsWidth != 400 {
			t.Fatalf("workspace constraints: %+v", layout)
		}
	})
}

func TestPanelHeader(t *testing.T) {
	for _, test := range []struct {
		name, size string
		height     float32
	}{{"default", "", 48}, {"table row", "row", 32}} {
		t.Run(test.name, func(t *testing.T) {
			native := NewNativeBinding(t.Context())
			tester := ui.NewTester(func(view *ui.Context) {
				theme.Apply(view, "", theme.Light)
				view.Root().Background(theme.Of(view).Muted)
				props, _ := json.Marshal([]any{"header", map[string]any{"size": test.size, "bordered": true, "surface": "content"}})
				ui.Column(view).Fill().Padding(8).Children(func() {
					header, err := native.renderLayout(view, NativeElement{Export: "PanelHeader", Props: props}, func() {
						ui.Row(view.Key("contents")).FillWidth().MinWidth(0).Children(func() {
							ui.Text(view, "Panel title").Grow(1).SingleLine()
							kit.IconButton(view.Key("close"), icons.Close, "Close", 28, false)
						})
					}, nil)
					if err != nil {
						t.Fatal(err)
					}
					header.Role(ui.RoleGroup).Label("Panel header")
				})
			}, 420, 220)
			tester.Frame()
			tester.Frame()
			group, exists := tester.AccessByRole(ui.RoleGroup, "Panel header")
			if !exists {
				t.Fatal("PanelHeader is missing")
			}
			bounds := group.Bounds
			button, ok := tester.AccessByRole(ui.RoleButton, "Close")
			if bounds.H != test.height || !ok || button.Bounds.H != 28 || button.Bounds.Y < bounds.Y || button.Bounds.Y+button.Bounds.H > bounds.Y+bounds.H {
				t.Fatalf("header=%v button=%v, want fixed height %v with fitting controls", bounds, button.Bounds, test.height)
			}
			frame := tester.Image()
			content := frame.RGBAAt(int(bounds.X+2), int(bounds.Y+2))
			for _, point := range [][2]int{{int(bounds.X), int(bounds.Y + 2)}, {int(bounds.X + bounds.W - 1), int(bounds.Y + 2)}, {int(bounds.X + 2), int(bounds.Y)}} {
				if pixel := frame.RGBAAt(point[0], point[1]); pixel != content {
					t.Fatal("PanelHeader has a top or side border")
				}
			}
			if frame.RGBAAt(int(bounds.X+2), int(bounds.Y+bounds.H-1)) == content || frame.RGBAAt(1, 1) == content {
				t.Fatal("PanelHeader is missing its bottom border or content surface")
			}
		})
	}
}

func TestHeaderKeepsValidContent(t *testing.T) {
	native := NewNativeBinding(t.Context())
	accepted, invalidations := "", 0
	native.Header = func(content json.RawMessage) error {
		var header map[string]any
		_ = json.Unmarshal(content, &header)
		if header["title"] == nil {
			return errors.New("invalid header title")
		}
		accepted = string(content)
		return nil
	}
	native.Invalidate = func() { invalidations++ }
	content := `{"title":"First"}`
	tester := ui.NewTester(func(view *ui.Context) {
		props, _ := json.Marshal([]any{"header", map[string]any{"content": content}})
		if _, err := native.renderHeader(view, NativeElement{Props: props}, nil, nil); err != nil {
			t.Fatal(err)
		}
	}, 300, 200)
	tester.Frame()
	for _, value := range []string{`{`, `{"actions":[]}`} {
		content = value
		tester.Frame()
		if accepted != `{"title":"First"}` || string(native.state.header) != accepted || invalidations != 1 {
			t.Fatal("invalid content replaced the last valid header")
		}
	}
	content = `{"title":"Second"}`
	tester.Frame()
	if accepted != content || invalidations != 2 {
		t.Fatal("a valid header did not recover after invalid content")
	}
}
