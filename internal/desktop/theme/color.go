package theme

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// Color is an optional package color: "#rgb", "#rgba", "#rrggbb" or
// "#rrggbbaa". Null or an omitted key leaves it unset, so the default
// applies.
type Color struct {
	V  ui.Color
	OK bool
}

// Or returns the color, or d where the package sets none.
func (c Color) Or(d ui.Color) ui.Color {
	if c.OK {
		return c.V
	}
	return d
}

// UnmarshalJSON reads a hex string or null.
func (c *Color) UnmarshalJSON(b []byte) error {
	if string(b) == "null" {
		*c = Color{}
		return nil
	}
	var s string
	if err := json.Unmarshal(b, &s); err != nil {
		return err
	}
	v, err := parseHex(s)
	if err != nil {
		return err
	}
	*c = Color{V: v, OK: true}
	return nil
}

// parseHex reads a CSS hex color without panicking, unlike ui.Hex.
func parseHex(s string) (ui.Color, error) {
	h, ok := strings.CutPrefix(strings.TrimSpace(s), "#")
	if !ok {
		return ui.Color{}, fmt.Errorf("color %q: want #rrggbb or #rrggbbaa", s)
	}
	switch len(h) {
	case 3, 4:
		var b strings.Builder
		for _, r := range h {
			b.WriteRune(r)
			b.WriteRune(r)
		}
		h = b.String()
	case 6, 8:
	default:
		return ui.Color{}, fmt.Errorf("color %q: want #rrggbb or #rrggbbaa", s)
	}
	if len(h) == 6 {
		h += "ff"
	}
	n, err := strconv.ParseUint(h, 16, 32)
	if err != nil {
		return ui.Color{}, fmt.Errorf("color %q: %w", s, err)
	}
	return ui.Color{R: uint8(n >> 24), G: uint8(n >> 16), B: uint8(n >> 8), A: uint8(n)}, nil
}
