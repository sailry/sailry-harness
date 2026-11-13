package plugins

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/MoeForge/moejs"
)

func TestMountedMemoryBytes(t *testing.T) {
	t.Run("retained JavaScript grows and shrinks without GC", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
export default class extends View {
  init(){this.values=null;}
  render(){return div()
    .child(new Button("retain").label("Retain").on_click(()=>{this.values=new Array(65536).fill(7);}))
    .child(new Button("release").label("Release").on_click(()=>{this.values=null;}));}
}`, RuntimeBindings{})
		snapshot, err := mounted.Snapshot()
		if err != nil {
			t.Fatal(err)
		}
		before, complete := mounted.MemoryBytes()
		if !complete || before == 0 {
			t.Fatalf("initial measurement = %d, complete %v", before, complete)
		}
		if err := mounted.Dispatch(snapshot.Children[0].Callbacks["on_click"], json.RawMessage(`{}`)); err != nil {
			t.Fatal(err)
		}
		retained, complete := mounted.MemoryBytes()
		if !complete || retained < before+256*1024 {
			t.Fatalf("retained array measurement = %d, complete %v, baseline %d", retained, complete, before)
		}
		if err := mounted.Dispatch(snapshot.Children[1].Callbacks["on_click"], json.RawMessage(`{}`)); err != nil {
			t.Fatal(err)
		}
		released, complete := mounted.MemoryBytes()
		if !complete || released+256*1024 > retained {
			t.Fatalf("released array measurement = %d, complete %v, retained %d", released, complete, retained)
		}
		mounted.Suspend()
		if bytes, complete := mounted.MemoryBytes(); !complete || bytes != 0 {
			t.Fatalf("suspended measurement = %d, complete %v", bytes, complete)
		}
	})
	t.Run("absent and closed engines have a complete zero", func(t *testing.T) {
		var absent *Mounted
		for _, mounted := range []*Mounted{absent, {}} {
			if bytes, complete := mounted.MemoryBytes(); !complete || bytes != 0 {
				t.Fatalf("absent measurement = %d, complete %v", bytes, complete)
			}
		}
		_, mounted := runtimeFixture(t, `import {View} from "gpui-kit";export default class extends View {render(){return "closed";}}`, RuntimeBindings{})
		if err := mounted.Close(); err != nil {
			t.Fatal(err)
		}
		if bytes, complete := mounted.MemoryBytes(); !complete || bytes != 0 {
			t.Fatalf("closed measurement = %d, complete %v", bytes, complete)
		}
	})
	t.Run("JavaScript cannot sample during its callback", func(t *testing.T) {
		var mounted *Mounted
		var bytes uint64
		complete, calls := true, 0
		_, mounted = runtimeFixture(t, `
import {View,div} from "gpui-kit";
import {Button} from "gpui-component";
import {sample} from "fixture";
export default class extends View {
  render(){return div().child(new Button("sample").label("Sample").on_click(()=>sample()));}
}`, RuntimeBindings{Modules: []HostModule{{Name: "fixture", Exports: []HostExport{{
			Name: "sample", Sync: func(context.Context, []json.RawMessage) (json.RawMessage, error) {
				calls++
				bytes, complete = mounted.MemoryBytes()
				return json.RawMessage(`null`), nil
			},
		}}}}})
		snapshot, err := mounted.Snapshot()
		if err != nil {
			t.Fatal(err)
		}
		if err := mounted.Dispatch(snapshot.Children[0].Callbacks["on_click"], json.RawMessage(`{}`)); err != nil {
			t.Fatal(err)
		}
		if calls != 1 || complete || bytes != 0 {
			t.Fatalf("reentrant sample = %d, complete %v, calls %d", bytes, complete, calls)
		}
		if bytes, complete := mounted.MemoryBytes(); !complete || bytes == 0 {
			t.Fatalf("between-call sample = %d, complete %v", bytes, complete)
		}
	})
	t.Run("measurement roots cannot reenter", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `import {View} from "gpui-kit";export default class extends View {render(){return "roots";}}`, RuntimeBindings{})
		calls, complete := 0, true
		var bytes uint64
		remove := mounted.engine.AddLiveRoots(func(func(moejs.Value)) {
			calls++
			bytes, complete = mounted.MemoryBytes()
		})
		defer remove()
		if bytes, complete := mounted.MemoryBytes(); !complete || bytes == 0 {
			t.Fatalf("outer sample = %d, complete %v", bytes, complete)
		}
		if calls != 1 || complete || bytes != 0 {
			t.Fatalf("nested sample = %d, complete %v, root calls %d", bytes, complete, calls)
		}
	})
	t.Run("interrupted walks publish no partial count", func(t *testing.T) {
		_, mounted := runtimeFixture(t, `
import {View,div} from "gpui-kit";
export default class extends View {
  init(props,cx){this.cx=cx;this.value="pending";}
  render(){
    if(!this.queued){this.queued=true;Promise.resolve().then(()=>{this.value="complete";this.cx.notify();});}
    return div().child(this.value);
  }
}`, RuntimeBindings{})
		snapshot, err := mounted.Snapshot()
		if err != nil || snapshot.Children[0].Text != "pending" || !mounted.jobsHeld {
			t.Fatalf("render did not retain its deferred job: %v", err)
		}
		remove := mounted.engine.AddLiveRoots(func(func(moejs.Value)) { mounted.engine.Interrupt(context.Canceled) })
		defer remove()
		if bytes, complete := mounted.MemoryBytes(); complete || bytes != 0 {
			t.Fatalf("interrupted sample = %d, complete %v", bytes, complete)
		}
		remove()
		if err := mounted.Pump(); err != nil {
			t.Fatal(err)
		}
		snapshot, err = mounted.Snapshot()
		if err != nil || snapshot.Children[0].Text != "complete" {
			t.Fatalf("sampling discarded the deferred job: %v", err)
		}
		if bytes, complete := mounted.MemoryBytes(); !complete || bytes == 0 {
			t.Fatalf("next sample = %d, complete %v", bytes, complete)
		}
	})
}
