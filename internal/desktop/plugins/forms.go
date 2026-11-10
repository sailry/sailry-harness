package plugins

import (
	"context"
	"encoding/json"
	"math"
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type textDraft struct {
	value       string
	label       string
	placeholder string
	multiline   bool
	rows        []int
	focus       bool
	focused     bool
	steps       float64
	input       ui.Element
}

type sliderDraft struct {
	value float64
	min   float64
	max   float64
	step  float64
	label string
}

func (binding *NativeBinding) formOperation(ctx context.Context, name string, arguments []json.RawMessage) (json.RawMessage, error) {
	if name == "nextTextEvent" {
		return binding.next(ctx, "text")
	}
	if name == "nextSliderEvent" {
		return binding.next(ctx, "slider")
	}
	if name == "createDateTime" || name == "readDateTime" || name == "releaseDateTime" {
		return binding.dateOperation(name, arguments)
	}
	identity := ""
	if name != "createText" && name != "createSlider" {
		var err error
		identity, err = hostValue[string](arguments, 0)
		if err != nil {
			return nil, err
		}
	}
	binding.state.mu.Lock()
	defer binding.state.mu.Unlock()
	switch name {
	case "createText":
		value, err := hostValue[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		options, err := hostOptional(arguments, 1, map[string]json.RawMessage{})
		if err != nil {
			return nil, err
		}
		rows := nativeProp(options, "rows", []int(nil))
		if len(rows) != 0 && (len(rows) != 2 || rows[0] <= 0 || rows[1] < rows[0]) {
			return nil, hostInvalid("text rows require positive ordered bounds")
		}
		if len(binding.state.text) >= 64 {
			return nil, hostInvalid("plugin field capacity exhausted")
		}
		binding.state.sequence++
		identity = "field-" + strconv.FormatUint(binding.state.sequence, 10)
		binding.state.text[identity] = &textDraft{
			value: value, label: nativeProp(options, "label", ""),
			placeholder: nativeProp(options, "placeholder", ""),
			multiline:   nativeProp(options, "multiline", false), rows: rows,
		}
		return hostJSON(identity)
	case "releaseText":
		delete(binding.state.text, identity)
		return json.RawMessage("null"), nil
	case "readText", "setText", "focusText", "isTextFocused":
		draft, exists := binding.state.text[identity]
		if !exists {
			return nil, hostInvalid("plugin field is unavailable")
		}
		switch name {
		case "readText":
			return hostJSON(draft.value)
		case "isTextFocused":
			return hostJSON(draft.focused)
		case "focusText":
			draft.focus = true
		case "setText":
			value, err := hostValue[string](arguments, 1)
			if err != nil {
				return nil, err
			}
			draft.value = value
		}
		return json.RawMessage("null"), nil
	case "createSlider":
		value, err := hostValue[float64](arguments, 0)
		if err != nil {
			return nil, err
		}
		options, err := hostValue[map[string]json.RawMessage](arguments, 1)
		if err != nil {
			return nil, err
		}
		value = float64(float32(value))
		minimum := float64(float32(nativeProp(options, "min", math.NaN())))
		maximum := float64(float32(nativeProp(options, "max", math.NaN())))
		step := float64(float32(nativeProp(options, "step", math.NaN())))
		if !finiteNumber(value) || !finiteNumber(minimum) || !finiteNumber(maximum) || !finiteNumber(step) || minimum >= maximum || step <= 0 {
			return nil, hostInvalid("slider requires finite ordered bounds and a positive step")
		}
		if len(binding.state.sliders) >= 64 {
			return nil, hostInvalid("plugin field capacity exhausted")
		}
		binding.state.sequence++
		identity = "slider-" + strconv.FormatUint(binding.state.sequence, 10)
		binding.state.sliders[identity] = &sliderDraft{
			value: min(max(value, minimum), maximum), min: minimum, max: maximum, step: step,
			label: nativeProp(options, "label", ""),
		}
		return hostJSON(identity)
	case "releaseSlider":
		delete(binding.state.sliders, identity)
		return json.RawMessage("null"), nil
	case "readSlider", "setSlider":
		draft, exists := binding.state.sliders[identity]
		if !exists {
			return nil, hostInvalid("plugin field is unavailable")
		}
		if name == "readSlider" {
			return hostJSON(draft.value)
		}
		value, err := hostValue[float64](arguments, 1)
		value = float64(float32(value))
		if err != nil || !finiteNumber(value) {
			return nil, hostInvalid("slider requires a finite value")
		}
		draft.value = min(max(value, draft.min), draft.max)
		return json.RawMessage("null"), nil
	}
	return nil, hostUnavailable("form operation is unavailable")
}

func (binding *NativeBinding) renderText(view *ui.Context, element NativeElement, children func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	binding.state.mu.Lock()
	draft := binding.state.text[identity]
	binding.state.mu.Unlock()
	if draft == nil {
		return ui.Box(view), nil
	}
	disabled := nativeProp(props, "disabled", false)
	readonly := nativeProp(props, "readonly", false)
	if draft.multiline && element.Export == "NumberField" {
		return ui.Box(view), nil
	}
	var input ui.Element
	number := element.Export == "NumberField"
	var renderErr error
	adornment := func() {
		if len(element.Children) == 0 {
			return
		}
		renderErr = renderNativeChildren(view, element, element.Children[:1])
	}
	container := ui.Row(view.Key(identity)).FillWidth().MinWidth(0).Gap(4)
	container.Children(func() {
		if nativeProp(props, "adornment", "") == "prefix" {
			adornment()
		}
		if draft.multiline {
			input = ui.TextArea(view.Key("input"), &draft.value)
			if len(draft.rows) == 2 {
				input.MinHeight(float32(draft.rows[0])*22 + 12).MaxHeight(float32(draft.rows[1])*22 + 12)
			}
		} else if !nativeProp(props, "appearance", true) {
			input = ui.TextInputBase(view.Key("input"), &draft.value)
		} else {
			input = ui.TextInput(view.Key("input"), &draft.value)
		}
		input.Grow(1).MinWidth(0)
		if nativeProp(props, "adornment", "") != "prefix" {
			adornment()
		}
		if number {
			draft.steps = 0
			ui.Stepper(view.Key("stepper"), &draft.steps, math.Inf(-1), math.Inf(1), 1).
				Label(draft.label).Disabled(disabled || readonly).OnChange(func() {
				kind := "increment"
				if draft.steps < 0 {
					kind = "decrement"
				}
				binding.emit("text", map[string]any{"id": identity, "kind": kind})
			})
		}
	})
	if renderErr != nil {
		return ui.Element{}, renderErr
	}
	draft.input = input
	input.Label(draft.label).Placeholder(draft.placeholder).FillWidth().Disabled(disabled).ReadOnly(readonly)
	if !nativeProp(props, "bordered", true) {
		input.BorderWidth(0)
	}
	if nativeProp(props, "size", "") == "small" {
		input.Height(28)
	}
	if height := nativeProp(props, "height", float32(0)); height > 0 {
		input.Height(height)
	}
	if draft.focus {
		input.Focus()
		draft.focus = false
	}
	focused := input.Focused()
	if focused != draft.focused {
		draft.focused = focused
		kind := "blur"
		if focused {
			kind = "focus"
		}
		binding.emit("text", map[string]any{"id": identity, "kind": kind})
	}
	input.OnChange(func() {
		binding.emit("text", map[string]any{"id": identity, "kind": "change"})
	})
	input.OnSubmit(func() {
		if !draft.multiline {
			binding.emit("text", map[string]any{"id": identity, "kind": "enter"})
		}
	})
	if element.Export == "NumberField" {
		input.OnTextKey(func(key ui.TextKey) bool {
			kind := ""
			switch key.Key {
			case ui.KeyUp:
				kind = "increment"
			case ui.KeyDown:
				kind = "decrement"
			}
			if kind != "" && !disabled && !readonly {
				binding.emit("text", map[string]any{"id": identity, "kind": kind})
				return true
			}
			return false
		})
	}
	if number {
		kit.Tooltip(view, container, draft.label)
	}
	return container, nil
}

func (binding *NativeBinding) renderSlider(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	binding.state.mu.Lock()
	draft := binding.state.sliders[identity]
	binding.state.mu.Unlock()
	if draft == nil {
		return ui.Box(view), nil
	}
	slider := ui.StepSlider(view.Key(identity), &draft.value, draft.min, draft.max, draft.step).
		Label(draft.label).FillWidth().Disabled(nativeProp(props, "disabled", false))
	slider.OnChange(func() {
		binding.emit("slider", map[string]any{"id": identity, "kind": "change", "value": draft.value})
	})
	if _, _, released := slider.DragEnded(); released || slider.Clicked() {
		binding.emitGuaranteed("slider", map[string]any{"id": identity, "kind": "release", "value": draft.value})
	}
	return slider, nil
}

func finiteNumber(value float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0)
}
