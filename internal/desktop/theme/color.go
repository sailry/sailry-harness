package theme

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

// Color is an optional package color in hex or Kit named-scale syntax.
// Null or an omitted key leaves it unset, so the default applies.
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

// UnmarshalJSON reads a color string or null.
func (c *Color) UnmarshalJSON(b []byte) error {
	if string(b) == "null" {
		*c = Color{}
		return nil
	}
	var s string
	if err := json.Unmarshal(b, &s); err != nil {
		return err
	}
	v, err := parseThemeColor(s)
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

// parseThemeColor preserves the source Kit named-scale and opacity syntax.
func parseThemeColor(value string) (ui.Color, error) {
	if strings.HasPrefix(value, "#") {
		return parseHex(value)
	}
	name, part, state := "", "", 0
	var scale uint64 = 500
	var opacity *float64
	for _, character := range value {
		switch {
		case character == '-' && state == 0:
			name, part, state = part, "", 1
		case character == '/' && state <= 1:
			if state == 0 {
				name = part
			} else if parsed, err := strconv.ParseUint(part, 10, 64); err == nil {
				scale = parsed
			}
			part, state = "", 2
		default:
			part += string(character)
		}
	}
	switch state {
	case 0:
		name = part
	case 1:
		if parsed, err := strconv.ParseUint(part, 10, 64); err == nil {
			scale = parsed
		}
	case 2:
		if parsed, err := strconv.ParseFloat(part, 32); err == nil {
			opacity = &parsed
		}
	}
	if name == "" {
		return ui.Color{}, fmt.Errorf("Empty color name")
	}
	var hex string
	switch strings.ToLower(name) {
	case "black":
		hex = "#000000"
	case "white":
		hex = "#ffffff"
	default:
		colors, known := namedColors[strings.ToLower(name)]
		if !known {
			return ui.Color{}, fmt.Errorf("Invalid color name")
		}
		hex = colors[scale]
		if hex == "" {
			hex = colors[500]
		}
	}
	color, err := parseHex(hex)
	if err != nil {
		return ui.Color{}, err
	}
	if opacity != nil {
		if *opacity > 100 {
			return ui.Color{}, fmt.Errorf("Invalid color opacity")
		}
		color = color.Alpha(float32(*opacity / 100))
	}
	return color, nil
}

// Background tokens use their first stop when projected into a solid palette,
// matching Kit's try_parse_theme_color behavior.
func parseThemeBackground(value string) (ui.Color, error) {
	if color, err := parseThemeColor(value); err == nil {
		return color, nil
	}
	value = strings.TrimSpace(value)
	const prefix = "linear-gradient("
	if !strings.HasPrefix(strings.ToLower(value), prefix) || !strings.HasSuffix(value, ")") {
		return ui.Color{}, fmt.Errorf("Unsupported background value")
	}
	parts := strings.Split(value[len(prefix):len(value)-1], ",")
	if len(parts) != 2 && len(parts) != 3 {
		return ui.Color{}, fmt.Errorf("Expected linear-gradient with two color stops")
	}
	if len(parts) == 3 {
		angle := strings.ToLower(strings.TrimSpace(parts[0]))
		if degrees, ok := strings.CutSuffix(angle, "deg"); ok {
			if _, err := strconv.ParseFloat(strings.TrimSpace(degrees), 32); err != nil {
				return ui.Color{}, err
			}
		} else {
			direction, ok := strings.CutPrefix(angle, "to ")
			if !ok {
				return ui.Color{}, fmt.Errorf("Unsupported linear-gradient angle: %s", angle)
			}
			var top, right, bottom, left bool
			for _, part := range strings.Fields(direction) {
				switch part {
				case "top":
					top = true
				case "right":
					right = true
				case "bottom":
					bottom = true
				case "left":
					left = true
				default:
					return ui.Color{}, fmt.Errorf("Unsupported linear-gradient direction: %s", direction)
				}
			}
			if top && bottom || left && right || !(top || right || bottom || left) {
				return ui.Color{}, fmt.Errorf("Unsupported linear-gradient direction: %s", direction)
			}
		}
		parts = parts[1:]
	}
	var first ui.Color
	for index, stop := range parts {
		fields := strings.Fields(stop)
		if len(fields) > 0 && strings.HasSuffix(fields[len(fields)-1], "%") {
			if _, err := strconv.ParseFloat(strings.TrimSuffix(fields[len(fields)-1], "%"), 32); err != nil {
				return ui.Color{}, err
			}
			fields = fields[:len(fields)-1]
		}
		color, err := parseThemeColor(strings.Join(fields, " "))
		if err != nil {
			return ui.Color{}, err
		}
		if index == 0 {
			first = color
		}
	}
	return first, nil
}

// Named returns a color of the named palette at scale, as "blue" at 600.
func Named(name string, scale uint64) (ui.Color, bool) {
	hex := namedColors[name][scale]
	if hex == "" {
		return ui.Color{}, false
	}
	color, err := parseHex(hex)
	return color, err == nil
}
