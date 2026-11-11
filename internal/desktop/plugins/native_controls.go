package plugins

import (
	"encoding/json"
	"math"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func (binding *NativeBinding) renderControl(view *ui.Context, element NativeElement, children func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	label := nativeProp(props, "label", "")
	disabled := nativeProp(props, "disabled", false)
	selected := nativeProp(props, "selected", false)
	send := func(value any) { binding.emit("control", map[string]any{"id": identity, "value": value}) }
	switch element.Export {
	case "IconButton":
		size := float32(28)
		if nativeProp(props, "size", "") == "medium" {
			size = 32
		}
		var button ui.Element
		if nativeProp(props, "show_label", false) {
			button = kit.Ghost(view, label, selected).Height(size).PaddingX(8).Gap(6).Children(func() {
				if icon := nativeIcon(nativeProp(props, "icon", "")); icon != nil {
					ui.Icon(view, icon).Size(16, 16)
				}
				nativeText(view, label).SingleLine()
				if nativeProp(props, "dropdown_caret", false) {
					ui.Icon(view, icons.ChevronDown).Size(12, 12)
				}
			})
		} else {
			button = kit.IconButtonHint(view, nativeIcon(nativeProp(props, "icon", "")), label, nativeProp(props, "shortcut", ""), size, selected)
		}
		if nativeProp(props, "variant", "") == "primary" {
			button.Background(theme.Of(view).ButtonPrimary).TextColor(theme.Of(view).OnPrimary)
		}
		if nativeProp(props, "circular", false) {
			button.Radius(size / 2)
		}
		if nativeProp(props, "full_width", false) {
			button.FillWidth()
		}
		button.Disabled(disabled).OnClick(func() { binding.emit("control", map[string]any{"id": identity}) })
		return button, nil
	case "Toggle":
		binding.state.mu.Lock()
		value := binding.state.booleans[identity]
		if value == nil {
			value = new(bool)
			binding.state.booleans[identity] = value
		}
		binding.state.mu.Unlock()
		*value = nativeProp(props, "checked", false)
		var control ui.Element
		switch nativeProp(props, "variant", "") {
		case "button":
			control = kit.Ghost(view, label, *value).Padding(6, 10).Children(func() { nativeText(view, nativeProp(props, "text", label)) })
			control.OnClick(func() { send(!*value) })
		case "checkbox":
			control = ui.Checkbox(view, value, "").Label(label).OnChange(func() { send(*value) })
		default:
			control = ui.Switch(view, value).Label(label).OnChange(func() { send(*value) })
		}
		return control.Disabled(disabled), nil
	case "SelectableRow":
		row := kit.Ghost(view, label, selected).FillWidth().MinWidth(0).Justify(ui.Start).Disabled(disabled).Children(children)
		row.OnClick(func() { binding.emit("control", map[string]any{"id": identity, "click_count": 1}) })
		return row, nil
	case "SelectableText":
		return kit.Markdown(view, nativeProp(props, "text", ""), kit.MarkdownProps{}), nil
	case "SelectField":
		return binding.renderSelect(view, identity, props)
	case "MenuButton":
		return binding.renderMenuButton(view, identity, props)
	case "Menu":
		return binding.renderMenu(view, identity, props)
	case "NativeContextMenu":
		return binding.renderContextMenu(view, identity, props, children)
	case "SegmentedTabs", "NavigationTabs":
		return binding.renderTabs(view, identity, props, element.Export == "NavigationTabs"), nil
	case "Measure":
		measure := ui.Box(view).FillWidth().Height(0).Draw(func(_ *ui.Painter, bounds ui.Rect) {
			width := uint32(min(max(math.Round(float64(bounds.W)), 0), float64(^uint32(0))))
			binding.state.mu.Lock()
			previous, exists := binding.state.measure[identity]
			binding.state.measure[identity] = width
			binding.state.mu.Unlock()
			if !exists || previous != width {
				send(width)
			}
		})
		return measure, nil
	case "LoadMore":
		loading := nativeProp(props, "loading", false)
		sentinel := ui.Row(view).FillWidth().Height(1).Center()
		if loading {
			sentinel.Height(32).Gap(8).Children(func() {
				kit.Busy(view)
				nativeText(view, label).TextColor(theme.Of(view).MutedForeground)
			})
		}
		visible := sentinel.InView()
		binding.state.mu.Lock()
		prior := binding.state.loadMore[identity]
		binding.state.loadMore[identity] = visible && !loading
		binding.state.mu.Unlock()
		if visible && !loading && !prior {
			send("load")
		}
		return sentinel, nil
	case "FocusEntry":
		var renderErr error
		region := ui.Column(view).Fill().MinWidth(0).MinHeight(0).TabStop(false).Children(func() {
			renderErr = renderNativeChildren(view, element, element.Children)
		})
		if renderErr != nil {
			return ui.Element{}, renderErr
		}
		binding.state.mu.Lock()
		focus := binding.state.focus == identity
		if focus {
			binding.state.focus = ""
		}
		binding.state.mu.Unlock()
		if focus {
			focused := false
			for _, child := range element.Children {
				if binding.focusNativeElement(child) {
					focused = true
					break
				}
			}
			if !focused {
				region.Focus()
			}
		}
		return region, nil
	case "ActionScope":
		region := ui.Column(view).Fill().MinWidth(0).Children(children)
		if close := nativeProp(props, "close", ""); close != "" && region.Shortcut(ui.Cmd, ui.KeyW) {
			binding.emit("control", map[string]any{"id": close})
		}
		return region, nil
	case "Appearance":
		value := nativeProp(props, "value", map[string]json.RawMessage{})
		// The reference's project icon: the glyph alone at 14, in its color.
		result := ui.Box(view).Size(14, 14).Shrink(0).Center()
		if color, err := nativeColor(nativeProp(value, "color", "")); err == nil {
			result.TextColor(color)
		}
		return result.Children(func() {
			if icon := nativeIcon(nativeProp(value, "icon", "")); icon != nil {
				ui.Icon(view, icon).Size(14, 14)
			}
		}), nil
	case "AppearancePicker":
		return binding.renderAppearance(view, identity, props)
	case "ModelPopup":
		return binding.renderModelPopup(view, identity, props)
	}
	return ui.Element{}, hostInvalid("unknown native control")
}

func (binding *NativeBinding) focusNativeElement(element *NativeElement) bool {
	if element == nil {
		return false
	}
	_, props, err := nativeProps(*element)
	if err == nil && nativeProp(props, "disabled", false) {
		return false
	}
	if element.Focus != nil && element.Focus() {
		return true
	}
	if element.Module == "sailry/forms" && (element.Export == "TextField" || element.Export == "NumberField") {
		identity, _, err := nativeProps(*element)
		if err == nil {
			binding.state.mu.Lock()
			draft := binding.state.text[identity]
			binding.state.mu.Unlock()
			if draft != nil {
				draft.input.Focus()
				return true
			}
		}
	}
	if element.Module == "sailry/credentials" && element.Export == "SecretField" {
		binding.state.mu.Lock()
		input, exists := binding.state.actionElements[element.ID]
		binding.state.mu.Unlock()
		if exists {
			input.Focus()
			return true
		}
	}
	for _, child := range element.Children {
		if binding.focusNativeElement(child) {
			return true
		}
	}
	return false
}
