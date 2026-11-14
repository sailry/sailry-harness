package plugins

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// ModelPopup is Kit's model select: it names a model as the application
// does, never by its id, and reports a choice as the package's menu did,
// the model's id with the effort as the catalog wrote it.
func TestModelPopup(t *testing.T) {
	const fast, plain = "0d6f1c1e-7a52-4f0e-9a57-3f1f3c1f0a11/gpt-5.5", "0d6f1c1e-7a52-4f0e-9a57-3f1f3c1f0a11/plain"
	props := map[string]json.RawMessage{}
	if err := json.Unmarshal([]byte(`{
		"label": "Model", "placeholder": "Choose a model", "selected": null,
		"catalog": {"models": [
			{"kind": "provider", "id": "`+fast+`", "provider": "OpenAI", "model": "gpt-5.5", "efforts": ["low", "medium", "high", {"budget": 1024}], "default_effort": "medium"},
			{"kind": "provider", "id": "`+plain+`", "provider": "OpenAI", "model": "plain"},
			{"kind": "agent", "id": "agent/one", "name": "An agent"}
		]}
	}`), &props); err != nil {
		t.Fatal(err)
	}
	native := NewNativeBinding(t.Context())
	native.Models = func() []kit.ModelOption {
		return []kit.ModelOption{{ID: fast, Name: "GPT-5.5", Provider: "OpenAI", Score: 71.5}}
	}
	var failure error
	tester := ui.NewTesterWith(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		ui.Column(view).Fill().Padding(20).Children(func() {
			if _, err := native.renderModelPopup(view, "model", props); err != nil && failure == nil {
				failure = err
			}
		})
	}, 700, 500, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.Frame()
	if failure != nil {
		t.Fatal(failure)
	}
	trigger, shown := tester.AccessByRole(ui.RoleButton, "Model")
	if !shown || trigger.HasPopup != ui.PopupListBox || !tester.HasText("Choose a model") {
		t.Fatalf("no model select with its placeholder: %+v %q", trigger, tester.Texts())
	}
	open := func() {
		t.Helper()
		tester.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
		tester.Frame()
		tester.Advance(time.Second)
	}
	click := func(option string) {
		t.Helper()
		row, listed := tester.AccessByRole(ui.RoleOption, option)
		if !listed {
			t.Fatalf("no option %q: %q", option, tester.Texts())
		}
		tester.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
		tester.Frame()
		tester.Advance(time.Second)
	}
	reported := func(want string) {
		t.Helper()
		// ModelSelect emits during the synchronous test frame. A missing
		// change is a failed interaction, not an asynchronous event to await.
		select {
		case event := <-native.queue("control", 32).events:
			if string(event) != want {
				t.Fatalf("reported %s, want %s", event, want)
			}
		default:
			t.Fatalf("model choice was not reported; want %s, shown %q", want, tester.Texts())
		}
	}
	open()
	// The application's name where it knows the model, the catalog's model
	// name where it does not, under the provider; no agent, no raw id.
	for _, text := range []string{"GPT-5.5", "plain", "OpenAI", "71.5"} {
		if !tester.HasText(text) {
			t.Errorf("the popover lacks %q: %q", text, tester.Texts())
		}
	}
	for _, text := range []string{fast, plain, "gpt-5.5", "An agent"} {
		if tester.HasText(text) {
			t.Errorf("the popover shows %q", text)
		}
	}
	// A model without efforts reports none and closes.
	click("plain")
	reported(`{"id":"model","value":{"effort":null,"model":"` + plain + `"}}`)
	if _, listed := tester.AccessByRole(ui.RoleOption, "plain"); listed {
		t.Error("the popover stays open after a model without efforts")
	}
	// A model with efforts is chosen at its default, and its efforts stay
	// beside it for another.
	props["selected"] = json.RawMessage(`"` + plain + `"`)
	open()
	click("GPT-5.5")
	reported(`{"id":"model","value":{"effort":"medium","model":"` + fast + `"}}`)
	props["selected"], props["effort"] = json.RawMessage(`"`+fast+`"`), json.RawMessage(`"medium"`)
	tester.Frame()
	click("high")
	reported(`{"id":"model","value":{"effort":"high","model":"` + fast + `"}}`)
	props["effort"] = json.RawMessage(`"high"`)
	tester.Frame()
	tester.Advance(time.Second)
	if trigger, _ = tester.AccessByRole(ui.RoleButton, "Model"); trigger.Value != "GPT-5.5 · high" {
		t.Errorf("the select reads %q", trigger.Value)
	}
	if _, listed := tester.AccessByRole(ui.RoleOption, "high"); listed {
		t.Error("the popover stays open after an effort")
	}
	// A token budget is offered by its number and reported as the catalog
	// wrote it.
	props["selected"] = json.RawMessage(`"` + plain + `"`)
	tester.Frame()
	open()
	click("GPT-5.5")
	reported(`{"id":"model","value":{"effort":"high","model":"` + fast + `"}}`)
	click("1024")
	reported(`{"id":"model","value":{"effort":{"budget":1024},"model":"` + fast + `"}}`)
}
