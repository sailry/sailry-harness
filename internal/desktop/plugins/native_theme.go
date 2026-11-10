package plugins

import (
	"encoding/json"
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func ThemeSnapshot(palette *theme.Palette) json.RawMessage {
	if palette == nil {
		palette = theme.Look("", false)
	}
	colors := map[string]ui.Color{
		"background": palette.Background, "foreground": palette.Foreground,
		"surface": palette.Popover, "surface_foreground": palette.PopoverForeground,
		"primary": palette.Primary, "primary_foreground": palette.PrimaryForeground,
		"secondary": palette.Secondary, "secondary_foreground": palette.SecondaryForeground,
		"muted": palette.Muted, "muted_foreground": palette.MutedForeground,
		"accent": palette.Hover, "accent_foreground": palette.HoverForeground,
		"destructive": palette.Danger, "destructive_foreground": palette.DangerForeground,
		"border": palette.Border, "input": palette.Input, "ring": palette.Ring,
		"selection": palette.Selection,
		"group_box": palette.Surface, "group_box_foreground": palette.SurfaceForeground,
		"success": palette.Success, "warning": palette.Warning, "info": palette.Info,
		"chart_1": ui.Hex("#93c5fd"), "chart_2": ui.Hex("#3b82f6"), "chart_3": ui.Hex("#2563eb"),
		"chart_4": ui.Hex("#1d4ed8"), "chart_5": ui.Hex("#1e40af"),
	}
	if palette.Variant != nil {
		for index := 1; index <= 5; index++ {
			if color, exists := palette.Variant.Theme.Colors[fmt.Sprintf("chart.%d", index)]; exists && color.OK {
				colors[fmt.Sprintf("chart_%d", index)] = color.V
			}
		}
	}
	encoded := make(map[string]string, len(colors))
	for name, color := range colors {
		encoded[name] = fmt.Sprintf("#%02x%02x%02x%02x", color.R, color.G, color.B, color.A)
	}
	value, _ := json.Marshal(map[string]any{"is_dark": palette.Dark, "colors": encoded})
	return value
}
