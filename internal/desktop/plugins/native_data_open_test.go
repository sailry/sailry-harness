package plugins

import (
	"encoding/json"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestTableOpen asserts a double click on a row sends open with its index.
func TestTableOpen(t *testing.T) {
	native := NewNativeBinding(t.Context())
	props := json.RawMessage(`["table",{"revision":"1","columns":["SQL"],"rows":[["SELECT 1"],["SELECT 2"]]}]`)
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Dark)
		ui.Column(view).Fill().Children(func() {
			if _, err := native.renderTable(view, NativeElement{ID: 1, Module: "sailry/ui", Export: "DataTable", Props: props}, nil, nil); err != nil {
				t.Fatal(err)
			}
		})
	}, 500, 300)
	tester.Frame()
	tester.Frame()
	row, ok := tester.Find("SELECT 2")
	if !ok {
		t.Fatalf("missing row: %q", tester.Texts())
	}
	tester.ClickAt(row.X+row.W/2, row.Y+row.H/2)
	tester.Frame()
	tester.ClickAt(row.X+row.W/2, row.Y+row.H/2)
	tester.Frame()
	for {
		select {
		case body := <-native.queue("table", 128).events:
			var event struct {
				Kind string
				Row  *int
			}
			if err := json.Unmarshal(body, &event); err != nil {
				t.Fatal(err)
			}
			if event.Kind == "open" {
				if event.Row == nil || *event.Row != 1 {
					t.Fatalf("open names row %v, want 1", event.Row)
				}
				return
			}
			continue
		default:
		}
		t.Fatal("a double click sent no open")
	}
}
