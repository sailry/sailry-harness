package plugins

import (
	"encoding/json"
	"fmt"
	"strconv"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func (mounted *Mounted) widgetKey(element *NativeElement) string {
	properties := runtimeProperties(element)
	identity := runtimeString(properties["id"])
	if identity == "" {
		identity = runtimeString(properties["motion_id"])
	}
	if identity == "" {
		if state, ok := properties["state"].(map[string]any); ok && runtimeFloat(state["__coreState"]) > 0 {
			identity = fmt.Sprintf("state:%v", state["__coreState"])
		}
	}
	if identity == "" {
		if ordinal := runtimeFloat(properties["__ordinal"]); ordinal > 0 {
			identity = fmt.Sprintf("position:%v", ordinal)
		} else {
			identity = strconv.FormatUint(element.ID, 10)
		}
	}
	return element.Module + "/" + element.Export + "/" + identity
}

func (mounted *Mounted) widgetID(element *NativeElement) uint64 {
	key := mounted.widgetKey(element)
	if identifier, exists := mounted.widgetIDs[key]; exists {
		return identifier
	}
	mounted.nextWidget++
	mounted.widgetIDs[key] = mounted.nextWidget
	return mounted.nextWidget
}

func (mounted *Mounted) emitValue(element *NativeElement, name string, value any) {
	if mounted.suspended {
		return
	}
	callback, exists := element.Callbacks[name]
	if !exists {
		return
	}
	data, err := json.Marshal(value)
	if err == nil {
		err = mounted.Dispatch(callback, data)
	}
	if err != nil {
		mounted.failure = err
	}
}

func (mounted *Mounted) renderTabs(context *ui.Context, element *NativeElement, properties map[string]any) (ui.Element, error) {
	if properties["menu"] == true {
		return ui.Element{}, fmt.Errorf("TabBar overflow menus are unavailable")
	}
	labels := make([]string, len(element.Children))
	for index, child := range element.Children {
		if child.Export != "Tab" {
			return ui.Element{}, fmt.Errorf("TabBar accepts only Tab children")
		}
		labels[index] = runtimeString(runtimeProperties(child)["label"])
	}
	identifier := mounted.widgetID(element)
	selected := mounted.selectors[identifier]
	if selected == nil {
		selected = new(int)
		mounted.selectors[identifier] = selected
	}
	*selected = int(runtimeFloat(properties["selected_index"]))
	var result ui.Element
	if runtimeString(properties["variant"]) == "segmented" {
		result = kit.Segmented(context, mounted.widgetKey(element), selected, labels, kit.SegmentedProps{})
	} else {
		result = ui.Tabs(context, selected, labels...)
	}
	result.OnChange(func() { mounted.emitValue(element, "on_change", *selected) })
	return result, nil
}

func (mounted *Mounted) renderRadioGroup(context *ui.Context, element *NativeElement, properties map[string]any) (ui.Element, error) {
	identifier := mounted.widgetID(element)
	selected := mounted.selectors[identifier]
	if selected == nil {
		selected = new(int)
		mounted.selectors[identifier] = selected
	}
	*selected = int(runtimeFloat(properties["selected_index"]))
	var renderErr error
	result := ui.RadioGroup(context, func() {
		for index, child := range element.Children {
			if child.Export != "Radio" {
				renderErr = fmt.Errorf("RadioGroup accepts only Radio children")
				return
			}
			properties := runtimeProperties(child)
			item := ui.Radio(context.Key(mounted.widgetKey(child)), selected, index, runtimeString(properties["label"]))
			item.OnChange(func() { mounted.emitValue(element, "on_change", *selected) })
			if err := mounted.applyStyles(context, item, child); err != nil {
				renderErr = err
				return
			}
		}
	})
	if element.Export == "HorizontalRadioGroup" {
		result.Row()
	} else {
		result.Column()
	}
	return result, renderErr
}

func (mounted *Mounted) renderField(context *ui.Context, element *NativeElement, properties map[string]any, children func()) (ui.Element, error) {
	label := runtimeString(properties["label"])
	if runtimeBool(properties["required"]) && label != "" {
		label += " *"
	}
	result := ui.Column(context).Gap(6)
	result.Children(func() {
		if label != "" {
			ui.Text(context, label).FontSize(14).FontWeight(500)
		}
		children()
		if description := runtimeString(properties["description"]); description != "" {
			ui.Text(context, description).FontSize(12).TextColor(theme.Of(context).MutedForeground)
		}
	})
	return result, nil
}

func (mounted *Mounted) renderStatusBar(context *ui.Context, element *NativeElement, children func()) (ui.Element, error) {
	result := ui.Row(context).AlignItems(ui.Center)
	var renderErr error
	slot := func(name string) {
		for _, child := range element.Slots[name] {
			if _, err := mounted.renderElement(context, child); err != nil && renderErr == nil {
				renderErr = err
			}
		}
	}
	result.Children(func() {
		slot("left_content")
		ui.Row(context).Grow(1).MinWidth(0).Children(children)
		slot("right_content")
	})
	return result, renderErr
}

func (mounted *Mounted) renderCollapsible(context *ui.Context, element *NativeElement, properties map[string]any, children func()) (ui.Element, error) {
	result := ui.Column(context)
	open := runtimeBool(properties["open"])
	var renderErr error
	result.Children(func() {
		children()
		target := float32(0)
		if open {
			target = 1
		}
		progress := result.Animate("reveal", target, 200*time.Millisecond)
		if !open && progress == 0 {
			return
		}
		panel := ui.Column(context).Shrink(0)
		var content ui.Element
		panel.Children(func() {
			content = ui.Column(context).Shrink(0).Children(func() {
				for _, child := range element.Slots["content"] {
					if _, err := mounted.renderElement(context, child); err != nil && renderErr == nil {
						renderErr = err
					}
				}
			})
		})
		if progress < 1 {
			panel.ClipY().Height(progress * content.Bounds().H)
		}
	})
	return result, renderErr
}

func (mounted *Mounted) renderAccordion(context *ui.Context, element *NativeElement, properties map[string]any) (ui.Element, error) {
	open := make([]*bool, len(element.Children))
	for index, child := range element.Children {
		if child.Export != "AccordionItem" {
			return ui.Element{}, fmt.Errorf("Accordion accepts only AccordionItem children")
		}
		identifier := mounted.widgetID(child)
		value := mounted.toggles[identifier]
		if value == nil {
			value = new(bool)
			mounted.toggles[identifier] = value
		}
		*value = runtimeBool(runtimeProperties(child)["open"])
		open[index] = value
	}
	parts := ui.AccordionBase(context)
	result := parts.Root
	if properties["bordered"] != false {
		result.Border(1, theme.Of(context).Border)
	}
	var renderErr error
	result.Children(func() {
		for index, child := range element.Children {
			item := ui.Column(context.Key(mounted.widgetKey(child)))
			item.Children(func() {
				section := parts.Item(open[index])
				section.Trigger.Disabled(runtimeBool(runtimeProperties(child)["disabled"]))
				section.Trigger.Children(func() {
					for _, title := range child.Slots["title"] {
						if _, err := mounted.renderElement(context, title); err != nil && renderErr == nil {
							renderErr = err
						}
					}
				})
				section.Trigger.OnChange(func() {
					if *open[index] && !runtimeBool(properties["multiple"]) {
						for other, value := range open {
							if other != index {
								*value = false
							}
						}
					}
					indices := []int{}
					for index, value := range open {
						if *value {
							indices = append(indices, index)
						}
					}
					mounted.emitValue(element, "on_toggle", indices)
				})
				section.Panel(func() {
					for _, nested := range child.Children {
						if _, err := mounted.renderElement(context, nested); err != nil && renderErr == nil {
							renderErr = err
						}
					}
				})
			})
			if err := mounted.applyStyles(context, item, child); err != nil && renderErr == nil {
				renderErr = err
			}
		}
	})
	return result, renderErr
}

func (mounted *Mounted) renderPagination(context *ui.Context, element *NativeElement, properties map[string]any) (ui.Element, error) {
	current, total, visible := 1, 1, 7
	if value := int(runtimeFloat(properties["current_page"])); value > 0 {
		current = value
	}
	if value := int(runtimeFloat(properties["total_pages"])); value > 0 {
		total = value
	}
	if value := int(runtimeFloat(properties["visible_pages"])); value > 0 {
		visible = value
	}
	result := ui.Row(context).Gap(4)
	result.Children(func() {
		previous := kit.IconButton(context, icons.ChevronLeft, "", 28, false).Disabled(current <= 1)
		previous.OnClick(func() { mounted.emitValue(element, "on_change", max(1, current-1)) })
		if !runtimeBool(properties["compact"]) {
			start := max(1, min(current-visible/2, total-visible+1))
			for page := start; page <= total && page < start+visible; page++ {
				button := kit.Ghost(context.Key(page), strconv.Itoa(page), page == current)
				button.OnClick(func() { mounted.emitValue(element, "on_change", page) })
			}
		}
		next := kit.IconButton(context, icons.ChevronRight, "", 28, false).Disabled(current >= total)
		next.OnClick(func() { mounted.emitValue(element, "on_change", min(total, current+1)) })
	})
	return result, nil
}

func (mounted *Mounted) renderDiff(context *ui.Context, element *NativeElement, properties map[string]any) (ui.Element, error) {
	state, _ := properties["state"].(map[string]any)
	patch := runtimeString(state["patch"])
	files, err := editor.ParsePatch(patch)
	if err != nil {
		return ui.Element{}, err
	}
	result := ui.Column(context).MinWidth(0).MinHeight(0)
	result.Children(func() {
		for index, file := range files {
			box := ui.Column(context.Key(index)).FillWidth().MinHeight(0)
			box.Children(func() {
				if properties["header_visible"] != false {
					name := file.NewName
					if name == "" {
						name = file.OldName
					}
					ui.Text(context, name).FontSize(12).TextColor(theme.Of(context).MutedForeground)
				}
				diff := *ui.Local(box, "patch", func() *editor.DiffState {
					value := &editor.DiffState{Layout: editor.DiffUnified, Indicators: editor.DiffClassic, FitContent: true}
					value.SetPatch(file)
					return value
				})
				if runtimeString(state["mode"]) == "split" {
					diff.Layout = editor.DiffSplit
				} else {
					diff.Layout = editor.DiffUnified
				}
				leaf := editor.DiffView(context, diff).FillWidth()
				if index == 0 {
					element.Focus = func() bool {
						leaf.Focus()
						return true
					}
				}
			})
		}
	})
	return result, nil
}
