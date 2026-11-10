package plugins

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

func (host *CardHost) Components() []NativeComponent {
	return []NativeComponent{
		{Module: "sailry/card", Export: "Action", Render: host.renderAction},
		{Module: "sailry/card", Export: "Qr", Render: host.renderQr},
	}
}

func (host *CardHost) renderAction(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	action, err := decodeCardAction(props["action"])
	if err != nil || !host.Permits(action) {
		return ui.Box(view).Size(0, 0), nil
	}
	label := nativeProp(props, "label", "")
	disabled := nativeProp(props, "disabled", false) || host.ctx.Err() != nil
	var button ui.Element
	if icon := nativeProp(props, "icon", ""); icon != "" {
		button = kit.IconButton(view, nativeIcon(icon), label, 28, false).Disabled(disabled)
	} else if nativeProp(props, "primary", false) {
		button = kit.Primary(view, label, disabled)
	} else {
		button = kit.Ghost(view, label, false).Disabled(disabled).Padding(6, 10)
	}
	if nativeProp(props, "icon", "") == "" {
		button.Children(func() { nativeText(view, label) })
	}
	button.OnClick(func() {
		if host.ctx.Err() == nil && host.binding.Action != nil && host.Permits(action) {
			host.binding.Action(identity, action)
		}
	})
	return button, nil
}
