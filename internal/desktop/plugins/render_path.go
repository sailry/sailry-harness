package plugins

import (
	"encoding/json"
	"fmt"
	"math"
	"strconv"
	"strings"

	"github.com/egoist/mygo/ui"
)

type nativePathCoordinate struct {
	value   float32
	percent bool
}

type nativePathCommand struct {
	name      string
	arguments []nativePathCoordinate
}

func (coordinate nativePathCoordinate) resolve(start, length float32) float32 {
	if coordinate.percent {
		return start + coordinate.value*length
	}
	return start + coordinate.value
}

func (mounted *Mounted) renderPath(context *ui.Context, element *NativeElement, properties map[string]any) (ui.Element, error) {
	var commands []nativePathCommand
	for _, style := range element.Styles {
		arity := 0
		switch style.Name {
		case "move_to", "line_to":
			arity = 2
		case "curve_to":
			arity = 4
		case "cubic_bezier_to":
			arity = 6
		case "close", "dash_array":
		default:
			continue
		}
		if len(style.Arguments) < arity {
			return ui.Element{}, fmt.Errorf("invalid path %s arguments", style.Name)
		}
		command := nativePathCommand{name: style.Name, arguments: make([]nativePathCoordinate, len(style.Arguments))}
		for index, argument := range style.Arguments {
			var value any
			if err := json.Unmarshal(argument, &value); err != nil {
				return ui.Element{}, err
			}
			switch value := value.(type) {
			case float64:
				if math.Abs(value) > math.MaxFloat32 {
					return ui.Element{}, fmt.Errorf("path coordinate exceeds native range")
				}
				command.arguments[index].value = float32(value)
			case string:
				if !strings.HasSuffix(value, "%") {
					return ui.Element{}, fmt.Errorf("invalid path coordinate %q", value)
				}
				number, err := strconv.ParseFloat(strings.TrimSuffix(value, "%"), 32)
				if err != nil {
					return ui.Element{}, err
				}
				command.arguments[index] = nativePathCoordinate{value: float32(number / 100), percent: true}
			default:
				return ui.Element{}, fmt.Errorf("invalid path coordinate type")
			}
		}
		commands = append(commands, command)
	}
	if runtimeString(properties["kind"]) != "solid" {
		return ui.Element{}, fmt.Errorf("path background %q is unavailable", runtimeString(properties["kind"]))
	}
	color, err := runtimeColor(context, runtimeString(properties["color"]))
	if err != nil {
		return ui.Element{}, err
	}
	color = color.Alpha(runtimeFloat(properties["opacity"]))
	width := runtimeFloat(properties["width"])
	fill := runtimeBool(properties["fill"])
	return ui.Box(context).Draw(func(painter *ui.Painter, bounds ui.Rect) {
		vector := &ui.Path{}
		for _, command := range commands {
			values := make([]float32, len(command.arguments))
			for index, argument := range command.arguments {
				if command.name == "dash_array" {
					values[index] = argument.value
				} else if index%2 == 0 {
					values[index] = argument.resolve(bounds.X, bounds.W)
				} else {
					values[index] = argument.resolve(bounds.Y, bounds.H)
				}
			}
			switch command.name {
			case "move_to":
				vector.MoveTo(values[0], values[1])
			case "line_to":
				vector.LineTo(values[0], values[1])
			case "curve_to":
				vector.QuadTo(values[2], values[3], values[0], values[1])
			case "cubic_bezier_to":
				vector.CubeTo(values[2], values[3], values[4], values[5], values[0], values[1])
			case "close":
				vector.Close()
			case "dash_array":
				vector.Dash(0, values...)
			}
		}
		if fill {
			painter.FillPath(vector, color)
		} else {
			painter.StrokePath(vector, width, color)
		}
	}), nil
}
