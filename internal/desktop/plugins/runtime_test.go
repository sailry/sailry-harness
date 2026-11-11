package plugins

import (
	"archive/zip"
	"context"
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/jonboulle/clockwork"
	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRuntimeLifecycle(t *testing.T) {
	t.Run("disconnect retains presentation without execution", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class extends View {
  init(){this.count=0;}
  render(){return div().child(new Button("retained").label("Retained "+this.count).on_click((event,cx)=>{this.count++;cx.notify();}));}
}`, RuntimeBindings{})
		var renderErr error
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			_, renderErr = mounted.Render(view)
		}, 420, 260)
		tester.Frame()
		if renderErr != nil || !tester.HasText("Retained 0") {
			t.Fatalf("initial presentation is missing: %v", renderErr)
		}
		callback := mounted.last.Children[0].Callbacks["on_click"]
		mounted.Suspend()
		tester.Frame()
		if renderErr != nil || !tester.HasText("Retained 0") || mounted.engine != nil || mounted.Pending() != 0 {
			t.Fatalf("disconnect lost the presentation or retained execution: %v", renderErr)
		}
		if err := mounted.Dispatch(callback, json.RawMessage(`{}`)); err == nil {
			t.Fatal("disconnected callback executed")
		}
		tester.Frame()
		if renderErr != nil || !tester.HasText("Retained 0") {
			t.Fatalf("late callback changed the retained presentation: %v", renderErr)
		}
	})
	t.Run("retained view and real frame events", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class Counter extends View {
  init() {this.count=0;}
  render() {return div().v_flex().child(new Button("counter").label("Count "+this.count).on_click((event,cx)=>{this.count++;cx.notify();}));}
}`, RuntimeBindings{})
		var renderErr error
		tester := ui.NewTester(func(context *ui.Context) {
			theme.Apply(context, "", theme.Light)
			_, renderErr = mounted.Render(context)
		}, 420, 260)
		tester.Frame()
		if renderErr != nil {
			t.Fatal(renderErr)
		}
		if err := tester.Click("Count 0"); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if renderErr != nil || !tester.HasText("Count 1") {
			t.Fatalf("persistent event did not update native frame: %v", renderErr)
		}
		snapshot, err := mounted.Snapshot()
		if err != nil {
			t.Fatal(err)
		}
		callback := snapshot.Children[0].Callbacks["on_click"]
		if err := mounted.Close(); err != nil {
			t.Fatal(err)
		}
		if err := mounted.Dispatch(callback, json.RawMessage(`{}`)); err == nil {
			t.Fatal("closed view accepted an old callback")
		}
	})
	t.Run("host promise and task continuation", func(t *testing.T) {
		started := make(chan struct{})
		release := make(chan struct{})
		posted := make(chan func(), 2)
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {load} from "fixture";
export default class Deferred extends View {
  init(props,cx) {this.value="pending";cx.spawn(async cx=>{this.value=await load();cx.notify();});}
  render() {return div().child(String(this.value));}
}`, RuntimeBindings{
			Post: func(function func()) { posted <- function },
			Modules: []HostModule{{Name: "fixture", Exports: []HostExport{{
				Name: "load", Async: func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
					close(started)
					select {
					case <-release:
						return json.RawMessage(`"ready"`), nil
					case <-ctx.Done():
						return nil, ctx.Err()
					}
				},
			}}}},
		})
		<-started
		before, err := mounted.Snapshot()
		if err != nil || before.Children[0].Text != "pending" {
			t.Fatalf("pending host request changed view prematurely: %v", err)
		}
		close(release)
		(<-posted)()
		after, err := mounted.Snapshot()
		if err != nil || after.Children[0].Text != "ready" || mounted.Pending() != 0 {
			t.Fatalf("host Promise did not resume the retained view: %v", err)
		}
	})
	t.Run("owned fake-clock timer", func(t *testing.T) {
		fake := clockwork.NewFakeClock()
		posted := make(chan func(), 2)
		runtime, err := NewRuntime(RuntimeBindings{Post: func(function func()) { posted <- function }})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = runtime.Close() })
		if err := runtime.SetClock(fake); err != nil {
			t.Fatal(err)
		}
		bundle, scope := runtimeBundle(`
import {View,div} from "gpui-kit";
export default class Timed extends View {
  init(props,cx) {this.value="before";cx.timer.after(25,cx=>{this.value="after";cx.notify();});}
  render() {return div().child(this.value);}
}`)
		mounted, err := runtime.Mount(bundle, scope)
		if err != nil {
			t.Fatal(err)
		}
		fake.Advance(25 * time.Millisecond)
		(<-posted)()
		snapshot, err := mounted.Snapshot()
		if err != nil || snapshot.Children[0].Text != "after" {
			t.Fatalf("owned timer did not resume on the VM owner: %v", err)
		}
	})
	t.Run("owner jobs stay outside render", func(t *testing.T) {
		started := make(chan struct{})
		release := make(chan struct{})
		posted := make(chan func(), 4)
		var phases []string
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {load,record} from "fixture";
export default class DeferredFrame extends View {
  init(props,cx) {
    this.cx=cx;this.value="pending";
    cx.spawn(async task=>{this.value=await load();record(task.phase());task.notify();});
  }
  render() {
    if(!this.queued) {this.queued=true;Promise.resolve().then(()=>record(this.cx.phase()));}
    return div().child(this.value);
  }
}`, RuntimeBindings{
			Post: func(function func()) { posted <- function },
			Modules: []HostModule{{Name: "fixture", Exports: []HostExport{
				{Name: "load", Async: func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
					close(started)
					select {
					case <-release:
						return json.RawMessage(`"ready"`), nil
					case <-ctx.Done():
						return nil, ctx.Err()
					}
				}},
				{Name: "record", Sync: func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
					var phase string
					if err := json.Unmarshal(arguments[0], &phase); err != nil {
						return nil, err
					}
					phases = append(phases, phase)
					return nil, nil
				}},
			}}},
		})
		<-started
		close(release)
		<-posted
		var renderErr error
		tester := ui.NewTester(func(context *ui.Context) {
			theme.Apply(context, "", theme.Light)
			_, renderErr = mounted.Render(context)
		}, 420, 260)
		tester.Frame()
		if renderErr != nil || !tester.HasText("ready") || len(phases) != 1 || phases[0] != "task" {
			t.Fatalf("queued host completion did not resume before render: phases=%v error=%v", phases, renderErr)
		}
		(<-posted)()
		if len(phases) != 2 || phases[1] != "task" {
			t.Fatalf("render Promise job did not resume on the next owner turn: %v", phases)
		}
	})
	t.Run("recursion leaves the host usable", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
export default class Recursive extends View {
  render() {const recur=()=>recur();return div().child(recur());}
}`, RuntimeBindings{})
		if _, err := mounted.Snapshot(); err == nil || !strings.Contains(err.Error(), "Maximum call stack size exceeded") {
			t.Fatalf("unbounded recursion did not return a stack fault: %v", err)
		}
		if err := mounted.Pump(); err != nil {
			t.Fatalf("the runtime was unusable after recursion stopped: %v", err)
		}
	})
	t.Run("child props and retired elements", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
class Child extends View {
  init(props) {this.value=props.value;}
  update(props) {this.value=props.value;}
  render() {return div().child(this.value);}
}
export default class Parent extends View {
  init(props,cx) {this.child=cx.new(Child,{value:"before"});}
  render() {return div().child(this.child).child(new Button("update").label("Update").on_click((event,cx)=>{this.child.set_props({value:"after"});cx.notify();}));}
}`, RuntimeBindings{})
		before, err := mounted.Snapshot()
		if err != nil || before.Children[0].Children[0].Text != "before" {
			t.Fatalf("child initialization failed: %v", err)
		}
		if err := mounted.Dispatch(before.Children[1].Callbacks["on_click"], nil); err != nil {
			t.Fatal(err)
		}
		after, err := mounted.Snapshot()
		if err != nil || after.Children[0].Children[0].Text != "after" {
			t.Fatalf("child props did not update: %v", err)
		}
		_, retired := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class Retired extends View {
  render() {
    const root=div();
    if(!this.saved) this.saved=div().child("saved");
    return root.child(this.saved).child(new Button("again").label("Again").on_click((event,cx)=>cx.notify()));
  }
}`, RuntimeBindings{})
		first, err := retired.Snapshot()
		if err != nil {
			t.Fatal(err)
		}
		if err := retired.Dispatch(first.Children[1].Callbacks["on_click"], nil); err != nil {
			t.Fatal(err)
		}
		if _, err := retired.Snapshot(); err == nil || !strings.Contains(err.Error(), "retired render generation") {
			t.Fatalf("a retired element was aliased by a new render: %v", err)
		}
	})
	t.Run("native child rendering and named actions", func(t *testing.T) {
		actions := map[string]func(){}
		keyContext := ""
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Shell} from "fixture";
export default class Commands extends View {
  init() {this.value="none";}
  render() {
    return Shell.new("shell").key_context("Fixture")
      .on_action("first",(_event,cx)=>{this.value="first";cx.notify();})
      .on_action("second",(_event,cx)=>{this.value="second";cx.notify();})
      .child(div().child(this.value));
  }
}`, RuntimeBindings{
			Components: []NativeComponent{{Module: "fixture", Export: "Shell", Render: func(context *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
				if element.RenderChild == nil || len(element.Children) != 1 {
					return ui.Element{}, errors.New("native child renderer is unavailable")
				}
				var renderErr error
				result := ui.Column(context).Children(func() {
					_, renderErr = element.RenderChild(context, element.Children[0])
				})
				return result, renderErr
			}}},
			BindActions: func(_ *ui.Context, _ ui.Element, element NativeElement, emit func(CallbackRef, json.RawMessage)) {
				if element.Export != "Shell" {
					return
				}
				for _, style := range element.Styles {
					if style.Name == "key_context" {
						if err := json.Unmarshal(style.Arguments[0], &keyContext); err != nil {
							t.Fatal(err)
						}
					}
				}
				for name, callback := range element.Callbacks {
					actions[name] = func() { emit(callback, nil) }
				}
			},
		})
		var renderErr error
		tester := ui.NewTester(func(context *ui.Context) {
			theme.Apply(context, "", theme.Light)
			_, renderErr = mounted.Render(context)
		}, 420, 260)
		tester.Frame()
		if renderErr != nil || keyContext != "Fixture" || !tester.HasText("none") {
			t.Fatalf("native children or action context were lost: context=%q error=%v", keyContext, renderErr)
		}
		for _, name := range []string{"first", "second"} {
			handler := actions["on_action:"+name]
			if handler == nil {
				t.Fatalf("named action %q was lost", name)
			}
			handler()
			tester.Frame()
			if renderErr != nil || !tester.HasText(name) {
				t.Fatalf("named action %q did not update the view: %v", name, renderErr)
			}
		}
	})
	t.Run("initialization failure aborts mount", func(t *testing.T) {
		runtime, err := NewRuntime(RuntimeBindings{
			Modules: []HostModule{{Name: "fixture", Exports: []HostExport{{
				Name: "fail", Sync: func(context.Context, []json.RawMessage) (json.RawMessage, error) {
					return nil, errors.New("fixture initialization failed")
				},
			}}}},
		})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = runtime.Close() })
		bundle, scope := runtimeBundle(`
import {View,div} from "gpui-kit";
import {fail} from "fixture";
export default class Failed extends View {
  init() {fail();}
  render() {return div().child("partially initialized");}
}`)
		if mounted, err := runtime.Mount(bundle, scope); mounted != nil || err == nil || !strings.Contains(err.Error(), "fixture initialization failed") {
			t.Fatalf("initialization failure did not abort mounting: mounted=%v error=%v", mounted != nil, err)
		}
		if len(runtime.mounts) != 0 {
			t.Fatal("failed initialization retained a mounted view")
		}
		bundle, scope = runtimeBundle(`import {View,div} from "gpui-kit"; export default class Ready extends View {render(){return div().child("ready");}}`)
		mounted, err := runtime.Mount(bundle, scope)
		if err != nil {
			t.Fatal(err)
		}
		snapshot, err := mounted.Snapshot()
		if err != nil || len(snapshot.Children) != 1 || snapshot.Children[0].Text != "ready" {
			t.Fatalf("failed initialization poisoned the next mount: %v", err)
		}
	})
	t.Run("unknown imports and methods fail", func(t *testing.T) {
		runtime, err := NewRuntime(RuntimeBindings{})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = runtime.Close() })
		bundle, scope := runtimeBundle(`import {View} from "missing"; export default class extends View {}`)
		if _, err := runtime.Mount(bundle, scope); err == nil {
			t.Fatal("unknown import was accepted")
		}
		_, mounted := runtimeFixture(t, `import {View,div} from "gpui-kit"; export default class extends View {render(){return div().imaginary_method();}}`, RuntimeBindings{})
		if _, err := mounted.Snapshot(); err == nil {
			t.Fatal("unknown element method was accepted")
		}
	})
}

func TestElementMapping(t *testing.T) {
	cases := []struct {
		name       string
		expression string
	}{
		{"layout", `div().v_flex().p_4().gap_2().child("Native text")`},
		{"button", `new Button("action").label("Native action").primary()`},
		{"checkbox", `new Checkbox("choice").label("Choice").checked(true)`},
		{"switch", `new Switch("switch").label("Enabled").checked(true)`},
		{"toggle", `new Toggle("toggle").label("Toggle").selected(true)`},
		{"progress", `new Progress("progress").value(30)`},
		{"spinner", `new Spinner("spinner")`},
		{"tag", `new Tag().child("Tag")`},
		{"field", `new Field().label("Field").required(true).child("Value")`},
		{"accordion", `new Accordion("accordion").child(new AccordionItem("item").child("Contents"))`},
		{"icon", `new Icon("settings").size("small")`},
		{"icon path", `new Icon("icons/chevron-left.svg").size("small")`},
		{"brand icon", `new Icon("ailogo:claude-color")`},
		{"brand icon in the text color", `new Icon("ailogo:openai")`},
		{"tabs", `new TabBar("tabs").selected_index(1).menu(false).children([new Tab().label("One"),new Tab().label("Two")])`},
		{"pagination", `new Pagination("pages").current_page(2).total_pages(5).visible_pages(3)`},
		{"shimmer", `new ShimmerText("Pending").id("pending")`},
		{"collapsible", `new Collapsible().open(true).motion_id("fold").content(div().child("Revealed"))`},
		{"border and corner styles", `div().border_1().border_t_0().border_y_1().rounded_t(3).w_px()`},
		{"path", `window.paint_path(PathBuilder.stroke(1).move_to(0,0).line_to("100%","100%").dash_array([4,28]).build(),Background.solid("#ff0000").opacity(0.5)).w(100).h(100)`},
	}
	for _, current := range cases {
		t.Run(current.name, func(t *testing.T) {
			_, mounted := runtimeFixture(t, `
import {View,div,PathBuilder,Background} from "gpui-kit";
import {Button,Checkbox,Switch,Toggle,Progress,Spinner,Tag,Field,Accordion,AccordionItem,Icon,Tab,TabBar,Pagination,ShimmerText,Collapsible} from "gpui-component";
export default class Example extends View {render(){return `+current.expression+`;}}`, RuntimeBindings{})
			var renderErr error
			tester := ui.NewTester(func(context *ui.Context) {
				theme.Apply(context, "", theme.Light)
				_, renderErr = mounted.Render(context)
			}, 480, 300)
			tester.Frame()
			if renderErr != nil {
				t.Fatal(renderErr)
			}
			if strings.HasPrefix(current.name, "brand icon") {
				// The mark draws: in its own color, or the text's.
				frame, colored, inked := tester.Image(), 0, 0
				for i := 0; i < len(frame.Pix); i += 4 {
					red, green, blue := int(frame.Pix[i]), int(frame.Pix[i+1]), int(frame.Pix[i+2])
					if red > 180 && red-blue > 80 && red-green > 60 {
						colored++
					}
					if red < 120 && green < 120 && blue < 120 {
						inked++
					}
				}
				if own := current.name == "brand icon"; own && colored < 20 || !own && (inked < 20 || colored != 0) {
					t.Fatalf("the brand mark drew %d pixels in its color and %d in the text's", colored, inked)
				}
			}
			if current.name == "shimmer" && !tester.HasText("Pending") {
				t.Fatal("the text constructor lost its content")
			}
			if _, err := mounted.Snapshot(); err != nil {
				t.Fatal(err)
			}
		})
	}
	t.Run("kit controls", testKitMapping)
	t.Run("plugin icon catalog", func(t *testing.T) {
		for _, name := range []string{
			"icons/chevron-down.svg", "icons/chevron-left.svg", "icons/chevron-right.svg",
			"icons/circle-alert.svg", "icons/close.svg", "icons/ellipsis-vertical.svg",
			"icons/ellipsis.svg", "icons/external-link.svg", "icons/folder.svg",
			"icons/globe.svg", "icons/hard-drive.svg", "icons/house.svg", "icons/network.svg",
			"icons/refresh-cw.svg", "icons/rotate-cw.svg", "icons/save.svg", "icons/square-terminal.svg",
			"reicon:arrows/arrow-up-right", "reicon:files/file-text", "reicon:it/terminal-square",
			"reicon:map/pointer", "reicon:newicons/edit", "reicon:time/calendar", "reicon:time/clock",
			"reicon:ui/alert-triangle", "reicon:ui/trash2", "reicon:video/play",
			"reicon:it/hashtag", "reicon:users/user", "reicon:newicons/hierarchy2", "reicon:money/coins",
			"reicon:map/globe", "reicon:ui/magnifier", "reicon:users/users2", "reicon:newicons/hierarchy",
			"reicon:business/diagram-tree", "reicon:video/image", "reicon:newicons/flash2",
			"reicon:building/buildings", "reicon:building/city", "reicon:business/chart-bar",
			"reicon:devices/cpu", "reicon:devices/monitor", "reicon:folders/folder", "reicon:it/code",
			"reicon:like/heart", "reicon:newicons/book-square", "reicon:newicons/cards", "reicon:newicons/grid2",
			"reicon:notifications/bell", "reicon:ui/box", "reicon:ui/crown", "reicon:ui/dark-light", "reicon:ui/database",
			"icons/remix/progress-4-line.svg", "icons/remix/progress-2-line.svg",
			"icons/remix/progress-6-line.svg", "icons/remix/close-circle-line.svg",
			"icons/remix/checkbox-blank-circle-line.svg", "calendar", "database", "play", "sliders-horizontal",
			"ailogo:openai", "ailogo:claude", "ailogo:claude-color", "ailogo:gemini-color", "ailogo:deepseek-text",
			"ailogo:alibaba-text-cn", "ailogo:alibaba-brand-color", "ailogo:ollama",
			"phosphor:database", "phosphor:puzzle-piece", "phosphor:slack-logo", "phosphor:git-pull-request",
		} {
			icon, err := icons.Plugin(name)
			if err != nil || icon == nil {
				t.Fatalf("declared plugin icon %q is unavailable: %v", name, err)
			}
			if nativeIcon(name) != icon {
				t.Fatalf("core and host resolve icon %q differently", name)
			}
		}
		// "reicon:it/window" is a name of the catalog whose glyph is not
		// embedded: nothing bundled names it.
		for _, unknown := range []string{"reicon:missing/icon", "reicon:it/window", "ailogo:missing-brand", "ailogo:openai-color", "ailogo:",
			"phosphor:missing", "phosphor:database-fill", "phosphor:", "phosphor:../database", "phosphor:Database"} {
			if _, err := icons.Plugin(unknown); err == nil {
				t.Fatalf("unknown plugin icon %q was accepted", unknown)
			}
		}
		if !icons.Colored("ailogo:claude-color") || icons.Colored("ailogo:claude") || icons.Colored("ailogo:openai-color") || icons.Colored("settings") {
			t.Fatal("the names of marks in their own colors are told apart wrongly")
		}
		// A tile shows a brand in its own colors where the collection has
		// them, whichever variant is named, and any other name has no mark.
		for name, want := range map[string]bool{"ailogo:figma": true, "ailogo:claude": true, "ailogo:claude-color": true, "ailogo:github": false, "ailogo:openai": false} {
			if mark, colored := icons.PluginBrand(name); mark == nil || colored != want {
				t.Fatalf("the tile's mark of %q: found %v, colored %v", name, mark != nil, colored)
			}
		}
		for _, name := range []string{"ailogo:missing-brand", "ailogo:", "phosphor:database", "reicon:time/calendar", "figma", ""} {
			if mark, _ := icons.PluginBrand(name); mark != nil {
				t.Fatalf("%q has a brand mark", name)
			}
		}
		// Every icon the bundled packages declare has its glyph. A package's
		// own icon, drawn on its tile, is a Phosphor icon, a brand's mark or
		// the package's SVG, and so may a connector's be; every other icon is
		// drawn in the text color beside the application's outline icons and
		// is neither.
		manifests, err := fs.Glob(sailry.Plugins, "plugins/*/plugin.json")
		if err != nil || len(manifests) < 40 {
			t.Fatalf("bundled manifests: %d, %v", len(manifests), err)
		}
		declared, own, chrome := map[string]bool{}, map[string]string{}, 0
		var walk func(manifest string, value any, tile bool)
		walk = func(manifest string, value any, tile bool) {
			switch value := value.(type) {
			case map[string]any:
				for key, child := range value {
					if name, named := child.(string); named && key == "icon" && strings.Contains(name, ":") {
						declared[name] = true
						if !tile {
							chrome++
							if strings.HasPrefix(name, "phosphor:") || strings.HasPrefix(name, "ailogo:") {
								t.Errorf("%s: %q is a tile's icon in a text-colored place", manifest, name)
							}
						}
					}
					walk(manifest, child, false)
				}
			case []any:
				for _, child := range value {
					walk(manifest, child, false)
				}
			}
		}
		for _, name := range manifests {
			data, err := fs.ReadFile(sailry.Plugins, name)
			if err != nil {
				t.Fatal(err)
			}
			var manifest struct {
				Extensions map[string]struct {
					Display struct {
						Icon json.RawMessage `json:"icon"`
					} `json:"display"`
				} `json:"extensions"`
			}
			var document struct {
				Extensions map[string]map[string]any `json:"extensions"`
			}
			if json.Unmarshal(data, &manifest) != nil || json.Unmarshal(data, &document) != nil {
				t.Fatalf("%s does not parse", name)
			}
			for field, value := range document.Extensions["dev.sailry.platform"] {
				switch connectors, listed := value.([]any); {
				case field == "display":
					walk(name, value, true)
				case field == "connectors" && listed:
					for _, connector := range connectors {
						for part, value := range connector.(map[string]any) {
							if part == "display" {
								walk(name, value, true)
							} else {
								walk(name, map[string]any{part: value}, false)
							}
						}
					}
				default:
					walk(name, map[string]any{field: value}, false)
				}
			}
			var icon protocol.PluginDesktopIcon
			if err := json.Unmarshal(manifest.Extensions["dev.sailry.platform"].Display.Icon, &icon); err != nil {
				t.Fatalf("%s declares no icon of its own: %v", name, err)
			}
			title, named := icon.Data.(*string)
			if named {
				own[path.Base(path.Dir(name))] = *title
			}
			if mark, _ := icons.PluginBrand(own[path.Base(path.Dir(name))]); mark == nil && !icon.Valid() {
				t.Errorf("%s: its own icon is neither declarable nor a brand's mark", name)
			}
			if named && !strings.HasPrefix(*title, "phosphor:") && !strings.HasPrefix(*title, "ailogo:") {
				t.Errorf("%s: its own icon %q is not a Phosphor icon or a brand's mark", name, *title)
			}
		}
		if len(declared) < 30 || chrome < 80 {
			t.Fatalf("bundled manifests declare %d icon names, %d of them in text-colored places", len(declared), chrome)
		}
		for name := range declared {
			if icon, err := icons.Plugin(name); err != nil || icon == nil {
				t.Errorf("bundled icon %q is unavailable: %v", name, err)
			}
		}
		for brand, slug := range map[string]string{"github": "github", "notion": "notion", "figma": "figma", "linear": "linear",
			"google-workspace": "google", "microsoft-365": "microsoft", "claude-auth": "claude", "openai-auth": "openai"} {
			if own[brand] != "ailogo:"+slug {
				t.Errorf("%s declares %q, not its brand's mark", brand, own[brand])
			}
		}
		// A manifest may name every icon of the embedded Phosphor set, and
		// only those: the protocol's list is the archive's names in order.
		listed, err := os.ReadFile(filepath.Join("..", "..", "protocol", "phosphor_icon_names.txt"))
		if err != nil {
			t.Fatal(err)
		}
		archive, err := zip.OpenReader(filepath.Join("..", "icons", "phosphor", "fill.zip"))
		if err != nil {
			t.Fatal(err)
		}
		defer archive.Close()
		var embedded []string
		for _, file := range archive.File {
			embedded = append(embedded, strings.TrimSuffix(file.Name, ".svg"))
		}
		if names := strings.Fields(string(listed)); len(names) != 1512 || !slices.IsSorted(names) || !slices.Equal(names, embedded) {
			t.Fatalf("the protocol lists %d Phosphor icons, the archive has %d", len(names), len(embedded))
		}
		for _, glyph := range embedded {
			name := "phosphor:" + glyph
			icon, err := icons.Plugin(name)
			if err != nil || icon == nil || nativeIcon(name) != icon || icons.ByName(name) != icon {
				t.Fatalf("the Phosphor icon %q is not drawn: %v", name, err)
			}
			if !(protocol.PluginDesktopIcon{Kind: "Name", Data: name}).Valid() {
				t.Fatalf("the Phosphor icon %q is not declarable", name)
			}
		}
		for _, unknown := range []string{"phosphor:missing", "phosphor:database-fill", "phosphor:", "ailogo:figma", "database"} {
			if (protocol.PluginDesktopIcon{Kind: "Name", Data: unknown}).Valid() {
				t.Fatalf("the protocol accepts the icon name %q", unknown)
			}
		}
	})
	t.Run("scope rejects foreign package", func(t *testing.T) {
		runtime, err := NewRuntime(RuntimeBindings{})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = runtime.Close() })
		bundle, scope := runtimeBundle(`export default class {}`)
		scope.Package.Name = "other"
		if _, err := runtime.Mount(bundle, scope); err == nil {
			t.Fatal("foreign package context was accepted")
		}
	})
}

func runtimeFixture(t *testing.T, source string, bindings RuntimeBindings) (*Runtime, *Mounted) {
	t.Helper()
	runtime, err := NewRuntime(bindings)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := runtime.Close(); err != nil && !errors.Is(err, context.Canceled) {
			t.Error(err)
		}
	})
	bundle, scope := runtimeBundle(source)
	mounted, err := runtime.Mount(bundle, scope)
	if err != nil {
		t.Fatal(err)
	}
	return runtime, mounted
}

func runtimeBundle(source string) (Bundle, PluginContext) {
	reference := protocol.PluginReference{Name: "runtime-fixture", Digest: strings.Repeat("a", 64)}
	bundle := Bundle{
		Package: reference, Entry: "dev.sailry.platform/desktop/main.js",
		Files:  map[string]string{"dev.sailry.platform/desktop/main.js": source},
		Images: map[string]string{},
	}
	scope := protocol.DefaultPluginContext()
	scope.Package = reference
	return bundle, scope
}
