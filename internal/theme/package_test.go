package theme

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestBuiltinPackagesParse(t *testing.T) {
	want := []string{BuiltinID, "great-sage", "holographic-rhythm", "ink-dragon"}
	if len(Packages) != len(want) {
		t.Fatalf("packages = %d, want %d", len(Packages), len(want))
	}
	ids := map[string]bool{}
	for _, p := range Packages {
		ids[p.ID] = true
	}
	for _, id := range want {
		if !ids[id] {
			t.Errorf("missing package %s", id)
		}
	}
	for _, p := range Packages[1:] {
		if p.Name == "" || p.Release == "" || p.License.Author == "" || p.License.Text == "" {
			t.Errorf("%s: incomplete manifest", p.ID)
		}
		for _, dark := range []bool{false, true} {
			v := p.Variant(dark)
			if len(v.Theme.Colors) == 0 {
				t.Errorf("%s dark=%v: no colors", p.ID, dark)
			}
			for slot, d := range v.Assets {
				if d.Bitmap() == nil {
					t.Errorf("%s dark=%v: asset %s (%s) does not decode", p.ID, dark, slot, d.Path)
				}
			}
		}
	}
}

func TestParseRequiresBothVariants(t *testing.T) {
	if _, err := Parse([]byte(`{"version":1,"id":"x","light":{"theme":{"colors":{}}}}`), nil); err == nil {
		t.Error("missing dark: want an error")
	}
	if _, err := Parse([]byte(`{"version":1,`), nil); err == nil {
		t.Error("malformed JSON: want an error")
	}
	if _, err := Parse([]byte(`{"version":1,"id":"x","light":{"theme":{"colors":{"background":"blue"}}},"dark":{}}`), nil); err == nil {
		t.Error("named color: want an error")
	}
}

func TestPaletteOverlaysNeutral(t *testing.T) {
	sage := Find("great-sage")
	p := palette(sage, false)
	if p.Background != ui.Hex("#F0E6D2") {
		t.Errorf("background = %v, want the package's", p.Background)
	}
	if p.Link != ui.Hex("#805914") {
		t.Errorf("link = %v, want the package's", p.Link)
	}
	// Not in the package: neutral values.
	if p.Selection != ui.Hex("#00000018") {
		t.Errorf("selection = %v, want neutral", p.Selection)
	}
	if p.Popover.A < 241 || p.Popover.A > 243 {
		t.Errorf("popover alpha = %d, want 95%%", p.Popover.A)
	}
	n := palette(Builtin, true)
	if n.Link != n.Primary || n.SwitchThumb != n.Background {
		t.Error("built-in: link and switch thumb fall back to primary and background")
	}
	if n.Variant != nil {
		t.Error("built-in: no variant")
	}
}
