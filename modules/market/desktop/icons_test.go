package desktop

import (
	"image"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

var testCatalog = i18n.New(Locales)

func TestDeclaredPackageIcons(t *testing.T) {
	rows := []preview.Package{
		{ID: "glyph-fixture", Name: "Glyph fixture", Icon: "phosphor:folder", Tint: "blue"},
		{ID: "brand-fixture", Name: "Brand fixture", Icon: "ailogo:figma", Tint: "purple"},
		{ID: "svg-fixture", Name: "SVG fixture", Icon: `<svg viewBox="0 0 24 24"><path d="M4 4h16v16H4z"/></svg>`, Tint: "green"},
		{ID: "uninstalled-fixture", Name: "Available fixture", Icon: "phosphor:puzzle-piece", Tint: "orange"},
	}
	for i := range rows {
		rows[i].Description, rows[i].Version, rows[i].Latest = "Local icon fixture", "1.0.0", "1.0.0"
		rows[i].Category, rows[i].Source = "market_category_development", "market_details_source_official"
		rows[i].Official, rows[i].Installable = true, true
		rows[i].Versions = []preview.PackageVersion{{Version: "1.0.0", Compatible: true}}
	}
	s := settings.New()
	s.Open(settings.Plugins)
	m := s.Model
	r := &renderer{}
	m.MarketPlugins = rows
	m.Packages = nil
	// Keep both installed and available rows; the glyph fixture is installed.
	for i, row := range rows {
		if i%3 != 0 || row.ID == "glyph-fixture" {
			row.Installed, row.Enabled = true, true
			m.Packages = append(m.Packages, row)
		}
	}
	page, mode := 0, theme.Dark
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		theme.Apply(c, "", mode)
		p := theme.Of(c)
		ui.Column(c).Fill().Background(p.Background).TextColor(p.Foreground).Children(func() {
			switch page {
			case 0:
				r.render(c, s.MarketScope())
			case 1:
				settings.Page(c, s, nil, settings.Look{})
			default:
				ui.Column(c.Key("details")).Width(520).Grow(1).Children(func() { settings.DetailsPanel(c, s) })
			}
		})
	}, 900, 2500, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("zh-CN")
	shots := t.TempDir()
	settle := func() {
		t.Helper()
		tt.Frame()
		for range 400 {
			if !tt.FramePending() {
				return
			}
			tt.Advance(50 * time.Millisecond)
			tt.Frame()
		}
		t.Fatalf("the window still asks for frames with nothing moving: %q", tt.Texts())
	}
	save := func(name string) {
		t.Helper()
		file, err := os.Create(filepath.Join(shots, name+".png"))
		if err != nil {
			t.Fatal(err)
		}
		defer file.Close()
		if err := png.Encode(file, tt.Image()); err != nil {
			t.Fatal(err)
		}
	}
	// where is the box of the icon tile: the leading 44 DIPs of the card's
	// summary button, or the 64 before the details' title.
	where := func(title string) ui.Rect {
		t.Helper()
		if page == 2 {
			at, found := tt.Find(title)
			if !found {
				t.Fatalf("no %q: %q", title, tt.Texts())
			}
			return ui.Rect{X: at.X - 72, Y: at.Y + at.H/2 - 30, W: 64, H: 60}
		}
		card, found := tt.AccessByRole(ui.RoleButton, title)
		if !found {
			t.Fatalf("no %q card: %q", title, tt.Texts())
		}
		return ui.Rect{X: card.Bounds.X - 4, Y: card.Bounds.Y, W: 44, H: card.Bounds.H}
	}
	// area is a box of the window in the last frame's pixels.
	area := func(at ui.Rect) (*image.RGBA, image.Rectangle) {
		img := tt.Image()
		scale := float32(img.Bounds().Dx()) / 900
		return img, image.Rect(int(at.X*scale), int(at.Y*scale), int((at.X+at.W)*scale), int((at.Y+at.H)*scale))
	}
	// mark is the pixels of the icon tile.
	mark := func(title string) string {
		t.Helper()
		settle()
		img, box := area(where(title))
		var pixels []byte
		for y := box.Min.Y; y < box.Max.Y; y++ {
			pixels = append(pixels, img.Pix[img.PixOffset(box.Min.X, y):img.PixOffset(box.Max.X, y)]...)
		}
		return string(pixels)
	}
	// vivid counts the pixels of a box that have a hue.
	vivid := func(at ui.Rect) int {
		img, box := area(at)
		count := 0
		for y := box.Min.Y; y < box.Max.Y; y++ {
			for x := box.Min.X; x < box.Max.X; x++ {
				if pixel := img.RGBAAt(x, y); max(pixel.R, pixel.G, pixel.B)-min(pixel.R, pixel.G, pixel.B) > 80 {
					count++
				}
			}
		}
		return count
	}
	subject := &m.MarketPlugins[0]
	for i := range m.MarketPlugins {
		if m.MarketPlugins[i].ID == "glyph-fixture" {
			subject = &m.MarketPlugins[i]
		}
	}
	installed := installedPackage(s.MarketScope(), subject.ID)
	if subject.Icon != "phosphor:folder" || installed == nil {
		t.Fatalf("the glyph fixture declares %q, installed %v", subject.Icon, installed != nil)
	}
	// brand is a package whose icon is a brand's mark in its own colors.
	var brand *preview.Package
	for i := range m.MarketPlugins {
		if m.MarketPlugins[i].ID == "brand-fixture" {
			brand = &m.MarketPlugins[i]
		}
	}
	if brand == nil || brand.Icon != "ailogo:figma" {
		t.Fatalf("the brand fixture declares %+v", brand)
	}
	for index, step := range []struct {
		name string
		of   *preview.Package
		open func()
	}{
		{"market", subject, func() { page = 0 }},
		{"plugins", installed, func() { page = 1 }},
		{"plugin-details", installed, func() {
			page = 2
			s.ShowDetails(settings.DetailPlugin, subject.ID)
		}},
	} {
		step.open()
		declared := step.of.Icon
		drawn := mark(step.of.Name)
		for _, dark := range []bool{true, false} {
			mode = map[bool]theme.Mode{true: theme.Dark, false: theme.Light}[dark]
			tt.SetDark(dark)
			settle()
			save(step.name + "-icons-zh-CN-" + map[bool]string{true: "dark", false: "light"}[dark])
		}
		mode = theme.Dark
		tt.SetDark(true)
		if again := mark(step.of.Name); again != drawn {
			t.Errorf("%s: the icon at rest changed between frames", step.name)
		}
		step.of.Icon = ""
		placeholder := mark(step.of.Name)
		if placeholder == drawn {
			t.Errorf("%s: the declared icon draws as the placeholder", step.name)
		}
		for _, unknown := range []string{"reicon:missing/icon", "phosphor:missing", "ailogo:missing-brand", "kanban"} {
			step.of.Icon = unknown
			if mark(step.of.Name) != placeholder {
				t.Errorf("%s: the unknown icon %q does not fall back to the placeholder", step.name, unknown)
			}
		}
		step.of.Icon = "phosphor:rocket"
		glyph := mark(step.of.Name)
		if glyph == placeholder || glyph == drawn {
			t.Errorf("%s: a Phosphor icon named by its id is not drawn", step.name)
		}
		step.of.Icon = "reicon:ui/puzzle-piece"
		if mark(step.of.Name) != placeholder {
			t.Errorf("%s: a Reicon glyph is not white on the tint's gradient", step.name)
		}
		step.of.Icon = `<svg viewBox="0 0 24 24"><path d="M4 4h16v16H4z"/></svg>`
		if own := mark(step.of.Name); own == placeholder || own == glyph {
			t.Errorf("%s: a package's own SVG is not drawn", step.name)
		}
		// A brand's mark is drawn, in the brand's colors where the
		// collection has them.
		step.of.Icon = "ailogo:figma"
		logo := mark(step.of.Name)
		step.of.Icon = "ailogo:github"
		mono := mark(step.of.Name)
		if logo == placeholder || mono == placeholder || logo == mono {
			t.Errorf("%s: a brand's mark is not drawn", step.name)
		}
		step.of.Icon = declared
		if index > 0 {
			continue
		}
		// A market card's tile is the 36 DIPs square at the start of its
		// summary button: inner is the 20 DIPs at its middle, where the
		// icon is, and edge the strip under its top, clear of the icon.
		tile := func(title string) (inner, edge ui.Rect) {
			at := where(title)
			middle := at.Y + at.H/2
			return ui.Rect{X: at.X + 12, Y: middle - 10, W: 20, H: 20}, ui.Rect{X: at.X + 16, Y: middle - 16, W: 12, H: 3}
		}
		// The glyph is on its tint's gradient, in no color of its own.
		// The declared icon is back: draw it before reading the tile.
		settle()
		inner, edge := tile(subject.Name)
		if vivid(edge) == 0 {
			t.Error("a glyph's tile has no tint")
		}
		// The pointer over the card moves nothing in its icon: the page
		// comes to rest (settle) and the tile keeps its pixels.
		pose := func() string {
			img, box := area(inner)
			var pixels []byte
			for y := box.Min.Y; y < box.Max.Y; y++ {
				pixels = append(pixels, img.Pix[img.PixOffset(box.Min.X, y):img.PixOffset(box.Max.X, y)]...)
			}
			return string(pixels)
		}
		rest := pose()
		tt.Move(inner.X+140, inner.Y+10)
		settle()
		if pose() != rest {
			t.Error("the icon of a hovered card is not its icon at rest")
		}
		tt.Move(2, 2)
		if mark(subject.Name) != drawn {
			t.Error("the icon changed after the pointer left its card")
		}
		// A brand's package has its mark on a neutral tile: in the
		// brand's colors, or in the foreground color for a mark without.
		for _, sample := range []struct {
			icon  string
			vivid bool
		}{{"ailogo:figma", true}, {"ailogo:github", false}, {"ailogo:missing-brand", false}} {
			brand.Icon = sample.icon
			settle()
			inner, edge := tile(brand.Name)
			if unknown := sample.icon == "ailogo:missing-brand"; (vivid(edge) != 0) != unknown {
				t.Errorf("%s: the tile's tint is %d pixels", sample.icon, vivid(edge))
			} else if !unknown && (vivid(inner) >= 20) != sample.vivid {
				t.Errorf("%s: the mark has %d pixels in a color of its own", sample.icon, vivid(inner))
			}
		}
		brand.Icon = "ailogo:figma"
	}
}
