package plugins

import (
	"bytes"
	"encoding/json"
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func (binding *NativeBinding) components() []NativeComponent {
	type renderer = func(*ui.Context, NativeElement, func(), func(CallbackRef, json.RawMessage)) (ui.Element, error)
	components := []NativeComponent{}
	add := func(module, name string, render renderer) {
		components = append(components, NativeComponent{Module: module, Export: name, Render: func(view *ui.Context, element NativeElement, children func(), emit func(CallbackRef, json.RawMessage)) (ui.Element, error) {
			binding.state.mu.Lock()
			failure := binding.state.failure
			binding.state.mu.Unlock()
			if failure != nil {
				return ui.Element{}, failure
			}
			return render(view, element, children, emit)
		}})
	}
	for _, name := range []string{"TextField", "NumberField"} {
		add("sailry/forms", name, binding.renderText)
	}
	add("sailry/forms", "DateTimeField", binding.renderDateTime)
	add("sailry/forms", "SliderField", binding.renderSlider)
	add("sailry/forms", "ConfigurationButton", binding.renderConfiguration)
	add("sailry/credentials", "SecretField", binding.renderSecret)
	add("sailry", "Header", binding.renderHeader)
	for _, name := range []string{
		"ActionScope", "FocusEntry", "IconButton", "Toggle", "SelectableRow", "SelectableText",
		"SelectField", "SegmentedTabs", "NavigationTabs", "MenuButton", "Menu", "NativeContextMenu",
		"Measure", "LoadMore", "Appearance", "AppearancePicker", "ModelPopup",
	} {
		add("sailry/ui", name, binding.renderControl)
	}
	for _, name := range []string{
		"SettingsPage", "SettingsGroup", "SettingsRow", "CardList", "CardRow", "CardSummary", "CardColumn",
		"PanelHeader", "EmptyState", "Loading", "InputSurface", "StatusList", "ScrollRegion", "TextLine",
		"Tooltip", "Modal", "Details", "Collapse", "CollapseSlot",
		"Workspace", "CardButton", "ComposedButton", "ClampedText", "HoverSwap", "ThinkingIcon", "StatusRing",
	} {
		add("sailry/ui", name, binding.renderLayout)
	}
	add("sailry/ui", "DataTable", binding.renderTable)
	add("sailry/ui", "ResourceTree", binding.renderTree)
	add("sailry/ui", "Picker", binding.renderPicker)
	for _, name := range []string{"Heatmap", "StackedChart", "ProgressBar"} {
		add("sailry/ui", name, binding.renderChart)
	}
	add("sailry/card", "Action", binding.renderCardAction)
	for _, entry := range []struct{ module, name string }{
		{"sailry", "Image"}, {"sailry", "Terminal"}, {"sailry", "BrowserAnnotationImage"}, {"sailry", "BrowserSurface"}, {"sailry", "Conversation"},
		{"sailry/ui", "FilePreview"}, {"sailry/documents", "DocumentSurface"}, {"sailry/artifacts", "ArtifactSurface"}, {"sailry/card", "Qr"},
	} {
		add(entry.module, entry.name, func(*ui.Context, NativeElement, func(), func(CallbackRef, json.RawMessage)) (ui.Element, error) {
			return ui.Element{}, hostUnavailable(fmt.Sprintf("%s.%s requires its native surface owner", entry.module, entry.name))
		})
	}
	return components
}

func nativeIcon(name string) *ui.SVG {
	icon, _ := icons.Plugin(name)
	return icon
}

func nativeText(view *ui.Context, text string) ui.Element {
	return ui.Text(view, text).FontSize(14)
}

func (binding *NativeBinding) renderHeader(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	_, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	content := nativeProp(props, "content", "")
	if len(content) > 2*1024*1024 || !json.Valid([]byte(content)) {
		return ui.Element{}, hostInvalid("invalid plugin header content")
	}
	binding.state.mu.Lock()
	changed := !bytes.Equal(binding.state.header, []byte(content))
	if changed {
		binding.state.header = append(binding.state.header[:0], content...)
	}
	binding.state.mu.Unlock()
	if changed && binding.Header != nil {
		if err := binding.Header(json.RawMessage(content)); err != nil {
			return ui.Element{}, err
		}
	}
	if changed && binding.Invalidate != nil {
		binding.Invalidate()
	}
	return ui.Box(view).Size(0, 0), nil
}

func (binding *NativeBinding) renderConfiguration(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	_, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	label := nativeProp(props, "label", "")
	available := binding.Operations["sailry/forms.openConfiguration"] != nil
	button := kit.Ghost(view, label, false).Padding(6, 10).Gap(6).
		Disabled(nativeProp(props, "disabled", false) || !available).Children(func() {
		ui.Icon(view, icons.Settings).Size(16, 16)
		nativeText(view, label).SingleLine()
	})
	button.OnClick(func() { binding.invoke("sailry/forms", "openConfiguration") })
	return button, nil
}

func nativeColor(value string) (ui.Color, error) {
	var color theme.Color
	body, _ := json.Marshal(value)
	if err := json.Unmarshal(body, &color); err != nil {
		return ui.Color{}, err
	}
	if !color.OK {
		return ui.Color{}, hostInvalid("invalid native color")
	}
	return color.V, nil
}

func (binding *NativeBinding) renderCardAction(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	label := nativeProp(props, "label", "")
	var button ui.Element
	if nativeProp(props, "primary", false) {
		button = kit.Primary(view, label, nativeProp(props, "disabled", false))
	} else {
		button = kit.Ghost(view, label, false).Disabled(nativeProp(props, "disabled", false)).Padding(6, 10)
	}
	button.Children(func() { nativeText(view, label) })
	button.OnClick(func() { binding.invoke("sailry/card", "action", identity, props["action"]) })
	return button, nil
}
