package theme

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestBuiltinPackagesParse(t *testing.T) {
	want := []string{BuiltinID, "great-sage", "holographic-rhythm", "ink-dragon"}
	if len(Packages()) != len(want) {
		t.Fatalf("packages = %d, want %d", len(Packages()), len(want))
	}
	ids := map[string]bool{}
	for _, p := range Packages() {
		ids[p.ID] = true
	}
	for _, id := range want {
		if !ids[id] {
			t.Errorf("missing package %s", id)
		}
	}
	for _, p := range Packages()[1:] {
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

// Installing replaces the set whole: a frame of another window reads the
// set before or after, a package that stays keeps its look, and a replaced
// look stays known to the window still showing it.
func TestSetInstalled(t *testing.T) {
	defer SetInstalled(nil)
	shipped := Widget("ink-dragon", true)
	added := &Package{ID: "added", Name: "Added"}
	newer := &Package{ID: "ink-dragon", Name: "Ink Dragon"}
	stop, done := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(done)
		for {
			select {
			case <-stop:
				return
			default:
				if Look("ink-dragon", true) == nil || Widget(BuiltinID, false) == nil || len(Packages()) < len(embeddedPackages) {
					panic("a reader saw an incomplete set")
				}
			}
		}
	}()
	for range 50 {
		SetInstalled([]*Package{added, newer})
		SetInstalled(nil)
	}
	SetInstalled([]*Package{added, newer})
	close(stop)
	<-done
	if got := Packages(); len(got) != len(embeddedPackages)+1 || got[len(got)-1] != added || Find("ink-dragon") != newer {
		t.Fatalf("installed set %v", got)
	}
	replaced, kept := Widget("ink-dragon", true), Widget("added", false)
	if replaced == shipped || current.Load().byWidget[shipped] == nil {
		t.Fatal("the replaced look is not known to the window showing it")
	}
	SetInstalled([]*Package{added})
	if Widget("added", false) != kept || Find("ink-dragon") == newer || Widget("ink-dragon", true) == replaced {
		t.Fatal("a staying package lost its look or a removed release stayed")
	}
}
