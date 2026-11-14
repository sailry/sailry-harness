package settings

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The terminal's font is chosen among the default and the system's
// monospaced families; a saved family the system does not list stays the
// choice shown.
func TestTerminalFontOptions(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Font = "No Such Mono"
	var options []string
	tt := ui.NewTesterWith(func(c *ui.Context) {
		options = nil
		for _, o := range fontOptions(c, m.Font) {
			options = append(options, o.Value)
		}
		terminal(c, m)
	}, 900, 900, ui.TesterOptions{})
	tt.SetLocale("en")
	tt.Frame()
	if len(options) < 2 || options[0] != "" || options[len(options)-1] != "No Such Mono" {
		t.Fatalf("font options = %q", options)
	}
	mono := map[string]bool{}
	for _, f := range ui.FontFamilies() {
		mono[f.Name] = f.Monospaced
	}
	for _, name := range options[1 : len(options)-1] {
		if !mono[name] {
			t.Errorf("%q is offered but is not a monospaced system family", name)
		}
	}
	if !tt.HasText("No Such Mono") {
		t.Errorf("the select does not show the saved font: %q", tt.Texts())
	}
	if _, ok := tt.AccessByRole(ui.RoleTextField, "Font"); ok {
		t.Error("the font is still typed in a text field")
	}
}
