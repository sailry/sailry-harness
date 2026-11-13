package plugins

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func (binding *NativeBinding) renderPath(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	segments := nativeProp(props, "segments", []string{})
	disabled := nativeProp(props, "disabled", false)
	// MyGo's Breadcrumbs makes the final segment noninteractive. CrychicUI's
	// Crumbs does that too, owns a non-Kit overflow menu and lets long labels
	// exceed the path's width (kit/crychic's Crumbs probe). Kit buttons keep
	// every segment actionable and truncate within the available row.
	// It reads as the path itself: the names in a row of plain text, a muted
	// "/" between them, each a ghost button that shows on hover.
	palette := theme.Of(view)
	path := ui.Row(view).FillWidth().MinWidth(0).Clip().AlignItems(ui.Center).Disabled(disabled)
	path.Children(func() {
		for index, segment := range segments {
			if index > 0 && segments[index-1] != "/" {
				ui.Text(view.Key(-index), "/").FontSize(13).Shrink(0).TextColor(palette.MutedForeground)
			}
			tone := palette.MutedForeground
			if index+1 == len(segments) {
				tone = palette.Foreground
			}
			button := kit.Ghost(view.Key(index), segment, false).Height(24).MinWidth(0).Shrink(1).Padding(0, 4).Disabled(disabled).Tooltip(segment)
			button.Children(func() { ui.Text(view, segment).FontSize(13).TextColor(tone).MinWidth(0).SingleLine() })
			button.OnClick(func() { binding.emit("control", map[string]any{"id": identity, "value": index}) })
		}
	})
	return path, nil
}
