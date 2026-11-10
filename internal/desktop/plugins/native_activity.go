package plugins

import (
	"encoding/json"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (binding *NativeBinding) renderActivityButton(view *ui.Context, element NativeElement, identity string, props map[string]json.RawMessage, children func()) ui.Element {
	palette := theme.Of(view)
	label := nativeProp(props, "label", "")
	button := kit.Ghost(view, label, false).FillWidth().MinWidth(0).Justify(ui.Start)
	if element.Export == "CardButton" {
		background := palette.Surface
		if palette.Dark {
			background = palette.Muted
		}
		if button.Pressed() {
			background = background.Over(palette.Active)
		} else if button.Hovered() {
			background = background.Over(palette.Hover)
		}
		button.Background(background).Padding(16).Border(1, palette.Border).Radius(palette.RadiusLg)
	} else {
		button.Height(32).PaddingX(4)
		if size := nativeProp(props, "size", float32(0)); finiteNumber(float64(size)) && size > 0 {
			button.Size(size, size).Padding(0)
		}
	}
	binding.state.hoverGroups[identity] = button.Hovered() || button.FocusWithin()
	button.Children(children).OnClick(func() { binding.emit("card", identity) })
	if tooltip := nativeProp(props, "tooltip", ""); tooltip != "" {
		kit.Tooltip(view, button, tooltip)
	}
	return button
}

func (binding *NativeBinding) renderStatusRing(view *ui.Context, props map[string]json.RawMessage) ui.Element {
	color, err := nativeColor(nativeProp(props, "color", ""))
	if err != nil {
		color = ui.Transparent
	}
	opacity := nativeProp(props, "opacity", float32(1))
	if !finiteNumber(float64(opacity)) {
		opacity = 1
	}
	color = color.Alpha(float32(color.A) / 255 * min(max(opacity, 0), 1))
	line := nativeProp(props, "line", float32(1.5))
	if !finiteNumber(float64(line)) || line < 0.5 || line > 8 {
		line = 1.5
	}
	running := nativeProp(props, "running", false)
	reduced := view.Preferences().ReduceMotion
	return ui.Box(view).Fill().Draw(func(painter *ui.Painter, bounds ui.Rect) {
		if bounds.W <= line || bounds.H <= line {
			return
		}
		stroke := func(start, fraction float64, tint ui.Color) {
			steps := max(2, int(math.Ceil(64*fraction)))
			var path ui.Path
			for index := 0; index <= steps; index++ {
				angle := (start + fraction*float64(index)/float64(steps)) * math.Pi * 2
				horizontal := bounds.X + bounds.W/2 + (bounds.W-line)/2*float32(math.Cos(angle))
				vertical := bounds.Y + bounds.H/2 + (bounds.H-line)/2*float32(math.Sin(angle))
				if index == 0 {
					path.MoveTo(horizontal, vertical)
				} else {
					path.LineTo(horizontal, vertical)
				}
			}
			if fraction >= 1 {
				path.Close()
			}
			painter.StrokePath(&path, line, tint)
		}
		track := color
		if running {
			track = track.Alpha(float32(track.A) / 255 * 0.35)
		}
		stroke(0, 1, track)
		if running {
			seconds := float64(painter.Now().UnixMilli()%600000) / 1000
			if !reduced {
				painter.AnimationFrameRate(kit.FPS)
			} else {
				seconds = 0
			}
			stroke(math.Mod(seconds/1.6, 1), 0.24, color)
		}
	})
}

func (binding *NativeBinding) EmitHeaderAction(identity string) {
	binding.emit("header", identity)
}

func (binding *NativeBinding) TakeDroppedFiles(identity string) ([]string, error) {
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	if err := binding.state.ctx.Err(); err != nil {
		return nil, err
	}
	paths, exists := binding.state.droppedFiles[identity]
	if !exists {
		return nil, hostInvalid("dropped files are unavailable")
	}
	delete(binding.state.droppedFiles, identity)
	return append([]string{}, paths...), nil
}

func (binding *NativeBinding) publishDroppedFiles(tree, target string, paths []string) {
	if len(paths) == 0 || binding.state.ctx.Err() != nil {
		return
	}
	identity := protocol.NewRequestID().String()
	binding.state.mu.Lock()
	binding.state.droppedFiles[identity] = append([]string{}, paths...)
	binding.state.mu.Unlock()
	body, err := hostJSON(map[string]any{"tree": tree, "id": target, "kind": "external_drop", "files": identity})
	if err != nil {
		return
	}
	queue := binding.queue("tree", 32)
	select {
	case queue.events <- body:
		if binding.Invalidate != nil {
			binding.Invalidate()
		}
	default:
		binding.state.mu.Lock()
		delete(binding.state.droppedFiles, identity)
		binding.state.mu.Unlock()
	}
}
