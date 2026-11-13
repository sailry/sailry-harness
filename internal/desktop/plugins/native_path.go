package plugins

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
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
	path := ui.Row(view).FillWidth().MinWidth(0).Clip().Gap(2).AlignItems(ui.Center).Disabled(disabled)
	path.Children(func() {
		for index, segment := range segments {
			if index > 0 {
				ui.Icon(view.Key(-index), icons.ChevronRight).Size(12, 12).Shrink(0).TextColor(theme.Of(view).MutedForeground)
			}
			button := kit.Ghost(view.Key(index), segment, false).MinWidth(0).Shrink(1).Padding(2, 6).Disabled(disabled).Tooltip(segment)
			tone := theme.Of(view).MutedForeground
			if index+1 == len(segments) {
				tone = theme.Of(view).Foreground
			}
			button.Children(func() { nativeText(view, segment).TextColor(tone).MinWidth(0).SingleLine() })
			button.OnClick(func() { binding.emit("control", map[string]any{"id": identity, "value": index}) })
		}
	})
	return path, nil
}
