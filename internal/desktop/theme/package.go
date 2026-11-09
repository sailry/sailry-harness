package theme

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io/fs"
	"path"
	"sort"

	"github.com/egoist/mygo/ui"
	sailry "github.com/sailry/sailry-harness"
)

// BuiltinID is the built-in default theme's identifier.
const BuiltinID = "builtin"

// Package is a v1 theme package (theme.json) with its assets.
type Package struct {
	Version     int    `json:"version"`
	ID          string `json:"id"`
	Name        string `json:"name"`
	Description string `json:"description"`
	// Release is the theme's own version, independent of the format's.
	Release  string   `json:"package_version"`
	Homepage string   `json:"homepage"`
	License  License  `json:"license"`
	Light    *Variant `json:"light"`
	Dark     *Variant `json:"dark"`
}

// License covers the package and the assets without one of their own.
type License struct {
	Author string `json:"author"`
	Name   string `json:"name"`
	Text   string `json:"text"`
}

// Variant is a package's light or dark half.
type Variant struct {
	Theme    Config                 `json:"theme"`
	Assets   map[string]*Decoration `json:"assets"`
	Composer Composer               `json:"composer"`
	Cards    map[string]Card        `json:"cards"`
	Messages map[string]Message     `json:"messages"`
	Wordmark Wordmark               `json:"wordmark"`
	Effects  map[string][]Effect    `json:"effects"`
}

// Config is the Kit theme: its colors by key and its radii.
type Config struct {
	Name           string           `json:"name"`
	IsDefault      bool             `json:"is_default"`
	FontSize       *float32         `json:"font.size"`
	FontFamily     *string          `json:"font.family"`
	MonoFontSize   *float32         `json:"mono_font.size"`
	MonoFontFamily *string          `json:"mono_font.family"`
	Shadow         *bool            `json:"shadow"`
	Highlight      json.RawMessage  `json:"highlight"`
	Mode           string           `json:"mode"`
	Colors         map[string]Color `json:"colors"`
	Radius         *float32         `json:"radius"`
	RadiusLg       *float32         `json:"radius.lg"`
}

// Composer styles the composer surface and its send and stop buttons.
type Composer struct {
	// BorderWidth draws the surface's edge in the input border color; zero
	// keeps the borderless surface.
	BorderWidth float32 `json:"border_width"`
	Send        *Action `json:"send"`
	Stop        *Action `json:"stop"`
}

// Action colors a filled button in its states.
type Action struct {
	Background Color `json:"background"`
	Foreground Color `json:"foreground"`
	Hover      Color `json:"hover"`
	Active     Color `json:"active"`
}

// Card styles a new-session mode card (explore, build, review, plan).
type Card struct {
	Background  Color    `json:"background"`
	Foreground  Color    `json:"foreground"`
	Detail      Color    `json:"detail"`
	Icon        Color    `json:"icon"`
	Hover       Color    `json:"hover"`
	Active      Color    `json:"active"`
	Border      Color    `json:"border"`
	BorderWidth float32  `json:"border_width"`
	Radius      *float32 `json:"radius"`
}

// Message styles the user's bubble or the assistant's text.
type Message struct {
	Background  Color    `json:"background"`
	Foreground  Color    `json:"foreground"`
	Border      Color    `json:"border"`
	BorderWidth float32  `json:"border_width"`
	Radius      *float32 `json:"radius"`
}

// Wordmark colors the welcome wordmark and its glow.
type Wordmark struct {
	Foreground Color `json:"foreground"`
	Glow       Color `json:"glow"`
}

// Fit is how a decoration fills its box.
type Fit string

const (
	Cover   Fit = "cover"
	Contain Fit = "contain"
	Fill    Fit = "fill"
	Tile    Fit = "tile"
)

// Decoration is a licensed raster picture in a slot (assets in theme.json).
type Decoration struct {
	Path string `json:"path"`
	Fit  Fit    `json:"fit"`
	// Alignment is horizontal and vertical, from 0 (start) to 1 (end).
	Alignment [2]float32 `json:"alignment"`
	Opacity   float32    `json:"opacity"`
	// Height is the banner, hero and button-edge height, or a corner's side.
	Height float32 `json:"height"`
	// TileSize is a tile's width; its aspect ratio is the picture's.
	TileSize float32  `json:"tile_size"`
	License  *License `json:"license"`

	pic *picture
}

// picture retains one decoded resource from the verified package snapshot.
type picture struct {
	bitmap *ui.Bitmap
}

// UnmarshalJSON applies the defaults to the omitted fields.
func (d *Decoration) UnmarshalJSON(b []byte) error {
	var decoded struct {
		Path      string     `json:"path"`
		Fit       Fit        `json:"fit"`
		Alignment [2]float32 `json:"alignment"`
		Opacity   float32    `json:"opacity"`
		Height    uint16     `json:"height"`
		TileSize  uint16     `json:"tile_size"`
		License   *License   `json:"license"`
	}
	decoded.Fit, decoded.Alignment, decoded.Opacity, decoded.Height, decoded.TileSize = Cover, [2]float32{0.5, 0.5}, 1, 64, 256
	if err := decodeThemeObject(b, &decoded, "path"); err != nil {
		return err
	}
	*d = Decoration{Path: decoded.Path, Fit: decoded.Fit, Alignment: decoded.Alignment, Opacity: decoded.Opacity, Height: float32(decoded.Height), TileSize: float32(decoded.TileSize), License: decoded.License}
	return nil
}

// Bitmap returns the resource decoded during strict package loading.
func (d *Decoration) Bitmap() *ui.Bitmap {
	if d == nil || d.pic == nil {
		return nil
	}
	return d.pic.bitmap
}

// Kind is an effect's motion.
type Kind string

const (
	Embers  Kind = "embers"
	Dust    Kind = "dust"
	Twinkle Kind = "twinkle"
	Pulse   Kind = "pulse"
	Petals  Kind = "petals"
	Scan    Kind = "scan"
)

// Effect is a layer of procedural particles on a surface.
type Effect struct {
	Kind    Kind    `json:"kind"`
	Color   Color   `json:"color"`
	Count   int     `json:"count"`
	Opacity float32 `json:"opacity"`
	Size    float32 `json:"size"`
	// Duration is one cycle in seconds.
	Duration float32 `json:"duration"`
	Seed     uint32  `json:"seed"`
}

// Asset returns the decoration in slot, or nil. A nil variant (the built-in
// theme) has none.
func (v *Variant) Asset(slot string) *Decoration {
	if v == nil {
		return nil
	}
	return v.Assets[slot]
}

// Layers returns the effect layers of a surface.
func (v *Variant) Layers(surface string) []Effect {
	if v == nil {
		return nil
	}
	return v.Effects[surface]
}

// Parse validates a current v1 manifest and a bounded snapshot of its images.
func Parse(data []byte, files fs.FS) (*Package, error) {
	if len(data) > themeManifestLimit {
		return nil, fmt.Errorf("theme manifest exceeds file limit")
	}
	var p Package
	if err := json.Unmarshal(data, &p); err != nil {
		return nil, fmt.Errorf("theme.json: %w", err)
	}
	if err := p.validate(); err != nil {
		return nil, err
	}
	if err := p.loadPictures(data, files); err != nil {
		return nil, err
	}
	return &p, nil
}

// Builtin is the default theme, which has no package: neutral colors, no
// assets, no effects.
var Builtin = &Package{ID: BuiltinID}

// Packages are the themes shipped: the built-in default first, then the
// official packages by name.
var Packages = load()

func load() []*Package {
	out := []*Package{Builtin}
	manifests, err := fs.Glob(sailry.Themes, "themes/*/theme.json")
	if err != nil {
		panic(err)
	}
	var found []*Package
	for _, manifest := range manifests {
		root, err := fs.Sub(sailry.Themes, path.Dir(manifest))
		if err != nil {
			panic(err)
		}
		data, err := fs.ReadFile(root, "theme.json")
		if err != nil {
			panic(err)
		}
		p, err := Parse(bytes.TrimSpace(data), root)
		if err != nil {
			panic(fmt.Sprintf("theme %s: %v", path.Base(path.Dir(manifest)), err))
		}
		found = append(found, p)
	}
	sort.Slice(found, func(i, j int) bool { return found[i].Name < found[j].Name })
	return append(out, found...)
}

// Find returns the package with id, or the built-in theme.
func Find(id string) *Package {
	for _, p := range Packages {
		if p.ID == id {
			return p
		}
	}
	return Builtin
}

// Variant returns the half for dark or light; nil for the built-in theme.
func (p *Package) Variant(dark bool) *Variant {
	if dark {
		return p.Dark
	}
	return p.Light
}

// Notice is the official packages' attribution (themes/NOTICE).
func Notice() string {
	b, _ := sailry.Themes.ReadFile("themes/NOTICE")
	return string(b)
}
