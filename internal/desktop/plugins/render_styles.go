package plugins

import (
	"encoding/json"
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// elementLook is the edges a host look gave an element before its package's
// styles: a style naming one edge keeps the others.
// A resizable group's axis, set on its panels before they draw.
const (
	panelHorizontal = iota + 1
	panelVertical
)

type elementLook struct {
	padding, border, radius [4]float32
}

func (mounted *Mounted) applyStyles(context *ui.Context, target ui.Element, element *NativeElement) error {
	var padding, margin, border, radius [4]float32
	if element.look != nil {
		padding, border, radius = element.look.padding, element.look.border, element.look.radius
	}
	fontSize := float32(14)
	// MyGo's Grow, also of no share, turns an unset basis into 0%, which
	// nothing sets back: a package's plain container that asks for no share
	// is left as it is built, without one, so that its basis stays its own
	// size. One that holds a scrolling list and names no minimum would
	// otherwise be laid out with no size in a parent of a known one.
	grown := element.Module != "gpui-kit" || element.Export != "Div"
	for _, style := range element.Styles {
		name := style.Name
		var argument any
		if len(style.Arguments) > 0 {
			if err := json.Unmarshal(style.Arguments[0], &argument); err != nil {
				return err
			}
		}
		if tail, exists := runtimeScaleStyle(name); exists {
			name, argument = tail.name, tail.value
		}
		if name == "size" && element.Export == "ResizablePanel" && element.panelAxis != 0 {
			// A panel's size is its extent along its group's axis.
			if extent, ok := argument.(float64); ok {
				if element.panelAxis == panelVertical {
					target.Height(float32(extent)).Shrink(0)
				} else {
					target.Width(float32(extent)).Shrink(0)
				}
				continue
			}
		}
		if name == "size" && element.Module == "gpui-component" {
			if label, ok := argument.(string); ok {
				sizes := map[string]float32{"xsmall": 12, "small": 16, "medium": 20, "large": 24}
				if size, exists := sizes[label]; exists {
					switch element.Export {
					case "Icon":
						target.Size(size, size)
					case "Button":
						// Kit's button heights: 20, 24, 32 and 36.
						target.Height(map[string]float32{"xsmall": 20, "small": 24, "medium": 32, "large": 36}[label])
						if size < 20 {
							padding[1], padding[3] = 8, 8
							target.Padding(padding[0], padding[1], padding[2], padding[3])
						}
					case "Tag", "Badge":
						// A tag is as high as its text: gpui-component's
						// sizes change its padding, not a control height,
						// so it centers in a row lower than a control.
					default:
						target.MinHeight(size + 12)
					}
					continue
				}
			}
		}
		value, percentage, automatic, err := runtimeLength(argument)
		if err != nil {
			value = 0
		}
		if automatic {
			value = ui.Auto
		}
		switch name {
		case "track_focus":
			if !mounted.suspended {
				target.TabStop(false).Bind(mounted.focusHandle(uint64(runtimeFloat(argument))))
			}
		case "w", "h", "size", "min_w", "min_h", "min_size", "max_w", "max_h", "max_size":
			if err != nil {
				return err
			}
			if automatic {
				// An automatic size is the element's own, as it is unset:
				// MyGo's Auto on a size stretches it to its limit instead.
				continue
			}
			if viewport, x, y := mounted.scrollViewport(); percentage && (x || y) {
				// A percentage of a scrolling axis, which MyGo leaves
				// unresolved, is that share of the scroll viewport.
				horizontal := name == "w" || name == "min_w" || name == "max_w"
				if name == "size" {
					if x {
						target.Width(viewport.W * value / 100)
					} else {
						target.WidthPercent(value)
					}
					if y {
						target.Height(viewport.H * value / 100)
					} else {
						target.HeightPercent(value)
					}
					continue
				}
				if horizontal && x || !horizontal && y {
					percentage = false
					if horizontal {
						value = viewport.W * value / 100
					} else {
						value = viewport.H * value / 100
					}
				}
			}
			if percentage {
				switch name {
				case "w":
					target.WidthPercent(value)
				case "h":
					target.HeightPercent(value)
				case "size":
					target.WidthPercent(value).HeightPercent(value)
				case "min_w":
					target.MinWidthPercent(value)
				case "min_h":
					target.MinHeightPercent(value)
				case "max_w":
					target.MaxWidthPercent(value)
				case "max_h":
					target.MaxHeightPercent(value)
				}
			} else {
				switch name {
				case "w":
					target.Width(value)
				case "h":
					target.Height(value)
				case "size":
					target.Size(value, value)
				case "min_w":
					target.MinWidth(value)
				case "min_h":
					target.MinHeight(value)
				case "min_size":
					target.MinWidth(value).MinHeight(value)
				case "max_w":
					target.MaxWidth(value)
				case "max_h":
					target.MaxHeight(value)
				case "max_size":
					target.MaxWidth(value).MaxHeight(value)
				}
			}
		case "p", "px", "py", "pt", "pr", "pb", "pl":
			runtimeEdges(&padding, name, value)
			target.Padding(padding[0], padding[1], padding[2], padding[3])
		case "m", "mx", "my", "mt", "mr", "mb", "ml":
			runtimeEdges(&margin, name, value)
			target.Margin(margin[0], margin[1], margin[2], margin[3])
		case "gap":
			target.Gap(value)
		case "gap_x":
			target.GapX(value)
		case "gap_y":
			target.GapY(value)
		case "inset", "top", "right", "bottom", "left":
			if percentage {
				switch name {
				case "inset":
					target.TopPercent(value).RightPercent(value).BottomPercent(value).LeftPercent(value)
				case "top":
					target.TopPercent(value)
				case "right":
					target.RightPercent(value)
				case "bottom":
					target.BottomPercent(value)
				case "left":
					target.LeftPercent(value)
				}
			} else {
				switch name {
				case "inset":
					target.Top(value).Right(value).Bottom(value).Left(value)
				case "top":
					target.Top(value)
				case "right":
					target.Right(value)
				case "bottom":
					target.Bottom(value)
				case "left":
					target.Left(value)
				}
			}
		case "flex", "flex_row", "h_flex":
			target.Row()
		case "flex_col", "v_flex":
			target.Column()
		case "flex_row_reverse":
			target.Row().Reverse()
		case "flex_col_reverse":
			target.Column().Reverse()
		case "flex_wrap":
			target.Wrap()
		case "flex_1", "flex_auto":
			// The reference's basis is 0%, not 0: nothing in a parent of a
			// known size, the content in one as high or wide as what it
			// holds. A basis of 0 there collapses a scrolling list to
			// nothing, as it has no minimum of its own.
			target.Grow(1).Shrink(1).BasisPercent(0)
			grown = true
		case "flex_none", "flex_initial":
			if grown {
				target.Grow(0)
			}
			if name == "flex_none" {
				target.Shrink(0)
			} else {
				target.Shrink(1)
			}
		case "flex_grow":
			if argument == nil {
				value = 1
			}
			target.Grow(value)
			grown = true
		case "flex_shrink", "flex_shrink_0":
			if argument == nil && name == "flex_shrink" {
				value = 1
			}
			target.Shrink(value)
		case "flex_basis":
			if automatic {
				continue
			}
			if percentage {
				target.BasisPercent(value)
			} else {
				target.Basis(value)
			}
		case "items_start", "items_center", "items_end", "items_stretch", "items_baseline":
			target.AlignItems(runtimeAlignment(strings.TrimPrefix(name, "items_")))
		case "justify_start", "justify_center", "justify_end", "justify_between", "justify_around", "justify_evenly":
			target.Justify(runtimeAlignment(strings.TrimPrefix(name, "justify_")))
		case "self_start", "self_center", "self_end", "self_stretch":
			target.AlignSelf(runtimeAlignment(strings.TrimPrefix(name, "self_")))
		case "absolute":
			target.Absolute()
		case "hidden":
			target.Hidden(true)
		case "invisible":
			target.Invisible()
		case "overflow_hidden":
			target.Clip()
		case "overflow_x_hidden":
			target.ClipX()
		case "overflow_y_hidden":
			target.ClipY()
		case "rounded", "rounded_t", "rounded_b", "rounded_l", "rounded_r", "rounded_tl", "rounded_tr", "rounded_bl", "rounded_br":
			runtimeCorners(&radius, name, value)
			target.Radius(radius[0], radius[1], radius[2], radius[3])
		case "rounded_none":
			radius = [4]float32{}
			target.Radius(0)
		case "rounded_sm":
			radius = [4]float32{4, 4, 4, 4}
			target.Radius(4)
		case "rounded_md":
			runtimeCorners(&radius, "rounded", theme.Of(context).Radius)
			target.Radius(theme.Of(context).Radius)
		case "rounded_lg":
			runtimeCorners(&radius, "rounded", theme.Of(context).RadiusLg)
			target.Radius(theme.Of(context).RadiusLg)
		case "rounded_xl", "rounded_2xl":
			runtimeCorners(&radius, "rounded", 12)
			target.Radius(12)
		case "rounded_full":
			runtimeCorners(&radius, "rounded", 9999)
			target.Radius(9999)
		case "bg":
			if background, ok := argument.(map[string]any); ok && runtimeBool(background["__background"]) {
				values, _ := background["values"].([]any)
				if len(values) != 0 {
					if runtimeString(background["kind"]) == "linear-gradient" && len(values) >= 5 {
						angle, _ := strconv.ParseFloat(runtimeString(values[0]), 32)
						first, firstErr := runtimeColor(context, runtimeString(values[1]))
						second, secondErr := runtimeColor(context, runtimeString(values[3]))
						if firstErr != nil || secondErr != nil {
							return fmt.Errorf("invalid plugin gradient colors")
						}
						target.Gradient(first, second, float32(angle))
					} else {
						color, colorErr := runtimeColor(context, runtimeString(values[0]))
						if colorErr != nil {
							return colorErr
						}
						target.Background(color)
					}
				}
			} else {
				color, colorErr := runtimeColor(context, runtimeString(argument))
				if colorErr != nil {
					return colorErr
				}
				target.Background(color)
			}
		case "text_color", "border_color", "color":
			color, colorErr := runtimeColor(context, runtimeString(argument))
			if colorErr != nil {
				return colorErr
			}
			if name == "text_color" || name == "color" {
				target.TextColor(color)
			} else {
				target.BorderColor(color)
			}
		case "border":
			border = [4]float32{value, value, value, value}
			target.BorderWidth(border[0], border[1], border[2], border[3])
		case "border_t":
			border[0] = value
			target.BorderWidth(border[0], border[1], border[2], border[3])
		case "border_r":
			border[1] = value
			target.BorderWidth(border[0], border[1], border[2], border[3])
		case "border_b":
			border[2] = value
			target.BorderWidth(border[0], border[1], border[2], border[3])
		case "border_l":
			border[3] = value
			target.BorderWidth(border[0], border[1], border[2], border[3])
		case "border_x":
			border[1], border[3] = value, value
			target.BorderWidth(border[0], border[1], border[2], border[3])
		case "border_y":
			border[0], border[2] = value, value
			target.BorderWidth(border[0], border[1], border[2], border[3])
		case "opacity":
			target.Opacity(value)
		case "text_size":
			fontSize = value
			target.FontSize(value)
		case "font_family":
			target.Font(runtimeString(argument))
		case "font_weight":
			target.FontWeight(int(value))
		case "font_thin", "font_extralight", "font_light", "font_normal", "font_medium", "font_semibold", "font_bold", "font_extrabold", "font_black":
			weights := map[string]int{"font_thin": 100, "font_extralight": 200, "font_light": 300, "font_normal": 400, "font_medium": 500, "font_semibold": 600, "font_bold": 700, "font_extrabold": 800, "font_black": 900}
			target.FontWeight(weights[name])
		case "text_xs", "text_sm", "text_base", "text_lg", "text_xl", "text_2xl", "text_3xl":
			sizes := map[string]float32{"text_xs": 12, "text_sm": 14, "text_base": 16, "text_lg": 18, "text_xl": 20, "text_2xl": 24, "text_3xl": 30}
			fontSize = sizes[name]
			target.FontSize(sizes[name])
		case "text_left", "text_center", "text_right":
			target.TextAlign(runtimeAlignment(strings.TrimPrefix(name, "text_")))
		case "whitespace_nowrap", "truncate", "text_ellipsis":
			target.SingleLine()
		case "selectable":
			if runtimeBool(argument) || argument == nil {
				target.Selectable()
			}
		case "disabled":
			target.Disabled(runtimeBool(argument))
		case "accessibility_label", "label":
			target.Label(runtimeString(argument))
		case "tooltip":
			kit.Tooltip(context, target, runtimeString(argument))
		case "id":
		case "motion_id", "required", "selected_index", "menu", "current_page", "total_pages", "visible_pages", "header_visible", "secondary", "success", "warning", "link":
		case "cursor_pointer":
			target.Cursor(ui.CursorPointer)
		case "cursor_text":
			target.Cursor(ui.CursorText)
		case "cursor_default":
			target.Cursor(ui.CursorDefault)
		case "line_height":
			if _, absolute := argument.(string); absolute && fontSize > 0 {
				value /= fontSize
			}
			target.LineHeight(value)
		case "italic":
			target.Italic()
		case "underline":
			target.Underline()
		case "line_through":
			target.Strikethrough()
		case "shadow_none":
			target.Shadow(0, 0, 0, 0, theme.Of(context).Foreground.Alpha(0))
		case "shadow_sm", "shadow_md", "shadow_lg", "shadow_xl":
			blur := map[string]float32{"shadow_sm": 3, "shadow_md": 8, "shadow_lg": 16, "shadow_xl": 24}[name]
			target.Shadow(0, blur/3, blur, 0, theme.Of(context).Foreground.Alpha(0.12))
		case "transition":
			if len(style.Arguments) < 2 {
				return fmt.Errorf("invalid plugin transition")
			}
			var property string
			var milliseconds float32
			if json.Unmarshal(style.Arguments[0], &property) != nil || json.Unmarshal(style.Arguments[1], &milliseconds) != nil {
				return fmt.Errorf("invalid plugin transition")
			}
			properties := runtimeProperties(element)
			styleName := map[string]string{"width": "w", "height": "h"}[property]
			if styleName == "" {
				styleName = property
			}
			current, _, _, lengthErr := runtimeLength(properties[styleName])
			if lengthErr != nil {
				return lengthErr
			}
			animated := target.Animate(property, current, time.Duration(milliseconds*float32(time.Millisecond)))
			switch property {
			case "opacity":
				target.Opacity(animated)
			case "width":
				target.Width(animated)
			case "height":
				target.Height(animated)
			case "left":
				target.Left(animated)
			case "top":
				target.Top(animated)
			}
		case "hover", "active", "focus":
			active := name == "hover" && target.Hovered() || name == "active" && target.Pressed()
			if active {
				for _, child := range element.Slots[name] {
					if err := mounted.applyStyles(context, target, child); err != nil {
						return err
					}
				}
			}
		case "relative", "block", "inline_block", "visible", "flex_nowrap", "whitespace_normal", "not_italic", "no_underline",
			"key_context", "aria_level", "keep_mounted", "selected", "checked", "role", "aria_selected", "aria_active_descendant",
			"window_control_area", "tab_index", "tab_stop", "href", "value", "start", "open", "default_open", "pressed", "indeterminate",
			"controls_right", "row_count", "column_count", "title", "description", "icon", "variant", "small", "xsmall", "large",
			"primary", "ghost", "outline", "danger", "loading", "compact", "text", "bordered", "closable", "appearance",
			"multiple", "collapsible", "resizable", "default_size", "axis", "position", "closeable",
			"line_numbers", "language", "placeholder", "show_whitespaces", "read_only", "soft_wrap", "auto_grow",
			"move_to", "line_to", "curve_to", "cubic_bezier_to", "close", "dash_array", "track_scroll",
			"content_focus_handle", "range_style", "cell_style", "spring", "frame_budget", "size_range", "panel_size", "panel_visible",
			"overflow_scroll", "overflow_x_scroll", "overflow_y_scroll", "overflow_scrollbar", "overflow_x_scrollbar", "overflow_y_scrollbar":
		default:
			return fmt.Errorf("plugin style %q has no native mapping", name)
		}
	}
	return nil
}

type runtimeScaledStyle struct {
	name  string
	value any
}

func runtimeScaleStyle(name string) (runtimeScaledStyle, bool) {
	for _, prefix := range []string{"border_t", "border_r", "border_b", "border_l", "border_x", "border_y", "border"} {
		if tail, exists := strings.CutPrefix(name, prefix+"_"); exists {
			if width, err := strconv.ParseFloat(tail, 32); err == nil {
				return runtimeScaledStyle{prefix, width}, true
			}
		}
	}
	for _, prefix := range []string{"min_w", "min_h", "max_w", "max_h", "gap_x", "gap_y", "w", "h", "p", "px", "py", "pt", "pb", "pl", "pr", "m", "mx", "my", "mt", "mb", "ml", "mr", "gap", "top", "bottom", "left", "right", "inset", "size"} {
		tail, exists := strings.CutPrefix(name, prefix+"_")
		if !exists {
			continue
		}
		if tail == "full" {
			return runtimeScaledStyle{prefix, "100%"}, true
		}
		if tail == "auto" {
			return runtimeScaledStyle{prefix, "auto"}, true
		}
		if tail == "px" {
			return runtimeScaledStyle{prefix, float64(1)}, true
		}
		value, err := strconv.ParseFloat(strings.ReplaceAll(tail, "p", "."), 32)
		if err == nil {
			return runtimeScaledStyle{prefix, float64(value * 4)}, true
		}
	}
	return runtimeScaledStyle{}, false
}

func runtimeLength(argument any) (float32, bool, bool, error) {
	if argument == nil {
		return 0, false, false, nil
	}
	switch value := argument.(type) {
	case float64:
		return float32(value), false, false, nil
	case string:
		if value == "auto" {
			return 0, false, true, nil
		}
		percentage := strings.HasSuffix(value, "%")
		scale := float64(1)
		if strings.HasSuffix(value, "rem") {
			scale = 16
			value = strings.TrimSuffix(value, "rem")
		}
		value = strings.TrimSuffix(strings.TrimSuffix(value, "px"), "%")
		number, err := strconv.ParseFloat(value, 32)
		if err != nil {
			return 0, false, false, fmt.Errorf("invalid plugin length %q", value)
		}
		return float32(number * scale), percentage, false, nil
	}
	return 0, false, false, fmt.Errorf("invalid plugin length")
}

func runtimeEdges(edges *[4]float32, name string, value float32) {
	switch name[1:] {
	case "":
		*edges = [4]float32{value, value, value, value}
	case "x":
		edges[1], edges[3] = value, value
	case "y":
		edges[0], edges[2] = value, value
	case "t":
		edges[0] = value
	case "r":
		edges[1] = value
	case "b":
		edges[2] = value
	case "l":
		edges[3] = value
	}
}

func runtimeCorners(corners *[4]float32, name string, value float32) {
	switch name {
	case "rounded":
		*corners = [4]float32{value, value, value, value}
	case "rounded_t":
		corners[0], corners[1] = value, value
	case "rounded_r":
		corners[1], corners[2] = value, value
	case "rounded_b":
		corners[2], corners[3] = value, value
	case "rounded_l":
		corners[0], corners[3] = value, value
	case "rounded_tl":
		corners[0] = value
	case "rounded_tr":
		corners[1] = value
	case "rounded_br":
		corners[2] = value
	case "rounded_bl":
		corners[3] = value
	}
}

func runtimeAlignment(name string) ui.Align {
	switch name {
	case "center":
		return ui.Center
	case "end", "right":
		return ui.End
	case "stretch":
		return ui.Stretch
	case "between":
		return ui.SpaceBetween
	case "around":
		return ui.SpaceAround
	case "evenly":
		return ui.SpaceEvenly
	case "baseline":
		return ui.Baseline
	}
	return ui.Start
}

func runtimeColor(context *ui.Context, name string) (ui.Color, error) {
	if strings.HasPrefix(name, "#") {
		value := strings.TrimPrefix(name, "#")
		if len(value) != 3 && len(value) != 4 && len(value) != 6 && len(value) != 8 {
			return ui.Color{}, fmt.Errorf("invalid plugin color %q", name)
		}
		if _, err := strconv.ParseUint(value, 16, 32); err != nil {
			return ui.Color{}, err
		}
		return ui.Hex(name), nil
	}
	palette := theme.Of(context)
	colors := map[string]ui.Color{
		"background": palette.Background, "foreground": palette.Foreground, "surface": palette.Surface,
		"group_box": palette.Surface, "border": palette.Border, "muted": palette.Muted, "muted_foreground": palette.MutedForeground,
		"primary": palette.Primary, "primary_foreground": palette.PrimaryForeground, "secondary": palette.Secondary,
		"destructive": palette.Danger, "destructive_foreground": palette.DangerForeground, "warning": palette.Warning,
		"success": palette.Success, "info": palette.Info, "popover": palette.Popover, "popover_foreground": palette.PopoverForeground,
		"surface_foreground": palette.SurfaceForeground, "group_box_foreground": palette.SurfaceForeground,
		"secondary_foreground": palette.SecondaryForeground, "accent": palette.Hover, "accent_foreground": palette.HoverForeground,
		"input": palette.Input, "ring": palette.Ring, "selection": palette.Selection,
		"transparent": palette.Background.Alpha(0),
	}
	if color, exists := colors[name]; exists {
		return color, nil
	}
	return ui.Color{}, fmt.Errorf("plugin semantic color %q is unavailable", name)
}
