package plugins

import (
	"encoding/json"
	"math"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestPathBar(t *testing.T) {
	t.Run("segments emit indices and disable together", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		disabled := false
		var failure error
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			props, _ := json.Marshal([]any{"path", map[string]any{"segments": []string{"/", "home", "project"}, "disabled": disabled}})
			_, failure = native.renderPath(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "PathBar", Props: props}, nil, nil)
		}, 400, 80)
		tester.Frame()
		for index, label := range []string{"/", "home", "project"} {
			if err := tester.Click(label); err != nil {
				t.Fatal(err)
			}
			select {
			case body := <-native.queue("control", 32).events:
				var event struct {
					ID    string `json:"id"`
					Value int    `json:"value"`
				}
				if err := json.Unmarshal(body, &event); err != nil || event.ID != "path" || event.Value != index {
					t.Fatalf("segment %d: %s, %v", index, body, err)
				}
			default:
				t.Fatalf("segment %d emitted no event", index)
			}
		}
		disabled = true
		tester.Frame()
		for _, label := range []string{"/", "home", "project"} {
			info, exists := tester.AccessByRole(ui.RoleButton, label)
			if !exists || !info.Disabled {
				t.Fatalf("disabled segment %q: %+v", label, info)
			}
			tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2)
		}
		select {
		case body := <-native.queue("control", 32).events:
			t.Fatalf("disabled path emitted %s", body)
		default:
		}
		if failure != nil {
			t.Fatal(failure)
		}
	})
	t.Run("long labels stay inside the path", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		var failure error
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			ui.Column(view).Width(240).Children(func() {
				_, failure = native.renderPath(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "PathBar",
					Props: json.RawMessage(`["path",{"segments":["/","a-very-long-home-directory-name","another-long-project-directory","current-directory-with-a-long-name"]}]`)}, nil, nil)
			})
		}, 400, 80)
		tester.Frame()
		tester.Frame()
		for _, info := range tester.AccessAll() {
			if info.Role == ui.RoleButton && (info.Bounds.X < -0.5 || info.Bounds.X+info.Bounds.W > 240.5 || math.IsNaN(float64(info.Bounds.W))) {
				t.Fatalf("segment exceeds path: %+v", info)
			}
		}
		if failure != nil {
			t.Fatal(failure)
		}
	})
	t.Run("script export accepts segments", func(t *testing.T) {
		native := NewNativeBinding(t.Context())
		components, err := NativeComponents(native)
		if err != nil {
			t.Fatal(err)
		}
		_, mounted := runtimeFixture(t, `
import {View} from "gpui-kit";
import {PathBar} from "sailry/ui";
export default class Example extends View {render(){return PathBar.new("path",{segments:["/","home","project"]});}}
`, RuntimeBindings{Components: components})
		root, err := mounted.Snapshot()
		if err != nil || root.Export != "PathBar" {
			t.Fatalf("PathBar export: %+v, %v", root, err)
		}
	})
}
