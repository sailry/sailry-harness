package theme

import (
	"bytes"
	"encoding/json"
	"image"
	"image/png"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestGalleryBitmapKeepsFullSizeArtworkSeparate(t *testing.T) {
	manifest, files := themeFixture(t)
	data, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, image.NewRGBA(image.Rect(0, 0, 1024, 256))); err != nil {
		t.Fatal(err)
	}
	files["assets/light.png"].Data = encoded.Bytes()
	pkg, err := Parse(data, files)
	if err != nil {
		t.Fatal(err)
	}
	art := pkg.Light.Asset("background.content")
	preview := art.PreviewBitmap()
	if width, height := preview.Size(); width != 768 || height != 192 {
		t.Fatalf("gallery bitmap is %dx%d", width, height)
	}
	if width, height := art.Bitmap().Size(); width != 1024 || height != 256 {
		t.Fatalf("full artwork is %dx%d", width, height)
	}
	if pkg.Light.Asset("brand").PreviewBitmap() != preview {
		t.Fatal("two slots sharing an asset decoded it twice")
	}
}

func TestBuiltinTheme(t *testing.T) {
	if got := Packages(); len(got) != 1 || got[0] != Builtin {
		t.Fatalf("default packages = %v, want only the built-in theme", got)
	}
	if Builtin.Preset() != DefaultSurfaces {
		t.Fatal("the built-in theme changed the default surfaces")
	}
	for _, dark := range []bool{false, true} {
		p, widget := Look("", dark), Widget("", dark)
		if p.Package != Builtin || p.Variant != nil || p.Dark != dark || widget.Dark != dark {
			t.Fatalf("dark=%v: the default appearance is not the built-in theme", dark)
		}
		if p.Background.A != 255 || p.Foreground.A == 0 || p.Primary.A == 0 || widget.Background != p.Background || widget.Text != p.Foreground {
			t.Fatalf("dark=%v: the built-in palette or widget theme is incomplete", dark)
		}
		if Find("not-installed") != Builtin || Look("not-installed", dark) != p || Widget("not-installed", dark) != widget {
			t.Fatalf("dark=%v: an unavailable theme does not fall back to the built-in theme", dark)
		}
	}
}

func TestPackagesParse(t *testing.T) {
	t.Run("Installed", func(t *testing.T) {
		manifest, files := themeFixture(t)
		data, err := json.Marshal(manifest)
		if err != nil {
			t.Fatal(err)
		}
		pkg, err := Parse(data, files)
		if err != nil {
			t.Fatal(err)
		}
		if pkg.Name == "" || pkg.Release == "" || pkg.License.Author == "" || pkg.License.Text == "" {
			t.Fatal("incomplete manifest")
		}
		for _, dark := range []bool{false, true} {
			v := pkg.Variant(dark)
			if len(v.Theme.Colors) == 0 {
				t.Errorf("dark=%v: no colors", dark)
			}
			for slot, d := range v.Assets {
				if d.Bitmap() == nil {
					t.Errorf("dark=%v: asset %s (%s) does not decode", dark, slot, d.Path)
				}
			}
		}
	})
	t.Run("RequiresBothVariants", func(t *testing.T) {
		if _, err := Parse([]byte(`{"version":1,"id":"x","light":{"theme":{"colors":{}}}}`), nil); err == nil {
			t.Error("missing dark: want an error")
		}
		if _, err := Parse([]byte(`{"version":1,`), nil); err == nil {
			t.Error("malformed JSON: want an error")
		}
		if _, err := Parse([]byte(`{"version":1,"id":"x","light":{"theme":{"colors":{"background":"blue"}}},"dark":{}}`), nil); err == nil {
			t.Error("named color: want an error")
		}
	})
}

func TestPaletteOverlaysNeutral(t *testing.T) {
	pkg := &Package{ID: "coast", Light: &Variant{Theme: Config{Colors: map[string]Color{
		"background": {V: ui.Hex("#edf5fb"), OK: true},
		"link":       {V: ui.Hex("#125e82"), OK: true},
	}}}}
	p := palette(pkg, false)
	if p.Background != ui.Hex("#edf5fb") {
		t.Errorf("background = %v, want the package's", p.Background)
	}
	if p.Link != ui.Hex("#125e82") {
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
	t.Run("chart colors", testChartColors)
}

// Installing replaces the set whole: a frame of another window reads the
// set before or after, a package that stays keeps its look, and a replaced
// look stays known to the window still showing it.
func TestSetInstalled(t *testing.T) {
	defer SetInstalled(nil)
	first := &Package{ID: "coast", Name: "Coast"}
	SetInstalled([]*Package{first})
	original, builtin := Widget("coast", true), Widget(BuiltinID, true)
	added := &Package{ID: "added", Name: "Added"}
	newer := &Package{ID: "coast", Name: "Coast", Release: "0.1.1"}
	stop, done := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(done)
		for {
			select {
			case <-stop:
				return
			default:
				if Look("coast", true) == nil || Widget("added", false) == nil || len(Packages()) < 1 {
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
	if got := Packages(); len(got) != 3 || got[0] != Builtin || got[1] != added || got[2] != newer || Find("coast") != newer {
		t.Fatalf("installed set %v", got)
	}
	replaced, kept := Widget("coast", true), Widget("added", false)
	old := retiredAppearance(original)
	if replaced == original || current.Load().byWidget[original] != nil || old.palette == nil || old.palette.Package != first {
		t.Fatal("the replaced look is not known to the window showing it")
	}
	if Widget(BuiltinID, true) != builtin {
		t.Fatal("installing packages replaced the built-in theme")
	}
	SetInstalled([]*Package{added})
	if Widget("added", false) != kept || Find("coast") != Builtin || Widget("coast", true) != builtin {
		t.Fatal("a staying package lost its look or a removed release stayed")
	}
}
