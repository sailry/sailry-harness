package plugins

import (
	"io/fs"
	"path"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestSavedConnectionList draws the landing page of the ssh and the databases
// package, each package's own view.js given two saved profiles and nothing
// open, and asserts the list under the New connection button: each profile
// is a card of its name and address, laid out inside the page and in order.
func TestSavedConnectionList(t *testing.T) {
	for _, current := range []struct {
		name, state, first, address, second, other, open string
	}{
		{"ssh", `terminals:[],terminal:null,jobs:[],activeProfile(){return null;},files(){return null;},
  profiles:[{id:"a",revision:1,name:"Build host",username:"root",host:"build.example",port:22},
    {id:"b",revision:1,name:"Mirror",username:"ops",host:"mirror.example",port:2222}]`,
			"Build host", "root@build.example:22", "Mirror", "ops@mirror.example:2222", `"id":"ssh-open-b"`},
		{"databases", `opened:null,catalog:{loading:new Set(),names:[]},profile(){return null;},
  profiles:[{id:"a",revision:1,name:"Ledger",connection:{kind:"sqlite",path:"/data/ledger.db"}},
    {id:"b",revision:1,name:"Archive",connection:{kind:"sqlite",path:"/data/archive.db"}}]`,
			"Ledger", "SQLite · /data/ledger.db", "Archive", "SQLite · /data/archive.db", `"id":"db-open-b"`},
	} {
		t.Run(current.name, func(t *testing.T) {
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
			bundle, scope := runtimeBundle("")
			directory := "dev.sailry.platform/desktop/"
			files, err := fs.Glob(sailry.Plugins, "plugins/"+current.name+"/"+directory+"*.js")
			if err != nil || len(files) == 0 {
				t.Fatalf("the package's desktop entry: %d files, %v", len(files), err)
			}
			delete(bundle.Files, bundle.Entry)
			for _, name := range files {
				body, err := fs.ReadFile(sailry.Plugins, name)
				if err != nil {
					t.Fatal(err)
				}
				bundle.Files[directory+path.Base(name)] = string(body)
			}
			// The package's view over a state the test owns: its own entry
			// would ask a Host for it.
			bundle.Entry = directory + "landing-fixture.js"
			bundle.Files[bundle.Entry] = `
import {View} from "gpui-kit";
import {messages} from "./locales.js";
import {render} from "./view.js";
export default class Example extends View {render(){return render({
  text:messages("en"),loaded:true,home:true,pending:null,outcome:null,editor:null,confirmation:null,fileEdit:null,transfer:null,
  commandOpen:false,picker:false,pickerProfile:null,pickerRevision:0,dialog:0,locked(){return false;},
  ` + current.state + `,
});}}`
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
				if !exists {
					t.Fatalf("%q is not shown: %q", text, tester.Texts())
				}
				if box.W <= 0 || box.H <= 0 || box.X < 0 || box.Y < 0 || box.X+box.W > 1024 || box.Y+box.H > 800 {
					t.Fatalf("%q is laid out at %v, outside the page", text, box)
				}
				return box
			}
			button := find("New connection")
			first, address := find(current.first), find(current.address)
			second := find(current.second)
			find(current.other)
			if first.Y < button.Y+button.H || second.Y < first.Y+first.H || first.X != button.X || first.W != button.W {
				t.Fatalf("the saved connections are not listed under the button: button %v, rows %v %v", button, first, second)
			}
			// A row is the reference's card: its address inside a border
			// and a padding of 12.
			if address.X < first.X+13 || address.Y < first.Y+13 || address.Y+address.H > first.Y+first.H-13+0.5 {
				t.Fatalf("the address %v is not inside its card %v", address, first)
			}
			// The rows are the page's and take its clicks.
			if err := tester.Click(current.second); err != nil {
				t.Fatal(err)
			}
			tester.Frame()
			event, err := native.next(t.Context(), "control")
			if err != nil || !strings.Contains(string(event), current.open) {
				t.Fatalf("a click on a saved connection reported %s, %v", event, err)
			}
		})
	}
}
