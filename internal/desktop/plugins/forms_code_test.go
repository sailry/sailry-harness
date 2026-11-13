package plugins

import (
	"encoding/json"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// TestCodeText asserts a multiline field with a language is a code editor
// that takes edits and sends "submit" on ⌘↵, and an unknown language is
// refused.
func TestCodeText(t *testing.T) {
	native := NewNativeBinding(t.Context())
	if _, err := native.formOperation(t.Context(), "createText", []json.RawMessage{json.RawMessage(`""`), json.RawMessage(`{"multiline":true,"language":"klingon"}`)}); err == nil {
		t.Fatal("an unknown language was accepted")
	}
	raw, err := native.formOperation(t.Context(), "createText", []json.RawMessage{json.RawMessage(`"SELECT 1"`), json.RawMessage(`{"multiline":true,"language":"sql","label":"SQL"}`)})
	if err != nil {
		t.Fatal(err)
	}
	var identity string
	if err := json.Unmarshal(raw, &identity); err != nil {
		t.Fatal(err)
	}
	props, _ := json.Marshal([]any{identity, map[string]any{}})
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Dark)
		ui.Column(view).Fill().Padding(16).Children(func() {
			if _, err := native.renderText(view, NativeElement{ID: 1, Module: "sailry/forms", Export: "TextField", Props: props}, nil, nil); err != nil {
				t.Fatal(err)
			}
		})
	}, 600, 400)
	tester.Frame()
	tester.Frame()
	info, ok := tester.Access("SQL")
	if !ok {
		t.Fatalf("no code editor: %q", tester.Texts())
	}
	tester.ClickAt(info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+10)
	tester.Frame()
	tester.Type(" ")
	tester.Frame()
	tester.Key(ui.Cmd, ui.KeyEnter)
	tester.Frame()
	kinds := map[string]bool{}
	for {
		select {
		case body := <-native.queue("text", 128).events:
			var event struct{ ID, Kind string }
			if err := json.Unmarshal(body, &event); err != nil || event.ID != identity {
				t.Fatalf("text event %s (%v)", body, err)
			}
			kinds[event.Kind] = true
			continue
		default:
		}
		break
	}
	if !kinds["change"] || !kinds["submit"] {
		t.Fatalf("events %v, want change and submit", kinds)
	}
	value, _ := native.formOperation(t.Context(), "readText", []json.RawMessage{json.RawMessage(`"` + identity + `"`)})
	if string(value) == `"SELECT 1"` {
		t.Fatalf("the edit did not reach the field: %s", value)
	}
}
