package plugins

import (
	"encoding/json"
	"io"
	"reflect"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type treeDraft struct {
	items    []nativeTreeItem
	cursor   string
	expanded map[string]*bool
}

type nativeTreeItem struct {
	ID         string                `json:"id"`
	Label      string                `json:"label"`
	Icon       string                `json:"icon"`
	Expanded   bool                  `json:"expanded"`
	Disabled   bool                  `json:"disabled"`
	Children   []nativeTreeItem      `json:"children"`
	Menu       []nativeMenuItem      `json:"menu"`
	Decoration *nativeTreeDecoration `json:"decoration"`
}

type nativeTreeDecoration struct {
	Selector string  `json:"selector"`
	Group    bool    `json:"group"`
	Detail   string  `json:"detail"`
	IconTone string  `json:"icon_tone"`
	IconSize float32 `json:"icon_size"`
	Badges   []struct {
		Text string `json:"text"`
		Tone string `json:"tone"`
	} `json:"badges"`
	BadgesWidth *float32 `json:"badges_width"`
	Check       *struct {
		ID       string `json:"id"`
		Label    string `json:"label"`
		Checked  bool   `json:"checked"`
		Mixed    bool   `json:"mixed"`
		Disabled bool   `json:"disabled"`
	} `json:"check"`
}

type nativeTreeDrag struct {
	owner *nativeState
	tree  string
	item  string
}

func (binding *NativeBinding) renderTree(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	source := nativeProp(props, "items", "")
	var items []nativeTreeItem
	decoder := json.NewDecoder(strings.NewReader(source))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&items) != nil || decoder.Decode(new(any)) != io.EOF || items == nil && strings.TrimSpace(source) != "[]" {
		return ui.Element{}, hostInvalid("invalid resource tree items")
	}
	selected := nativeProp(props, "selected", []string{})
	current := nativeProp(props, "current", "")
	variant := nativeProp(props, "variant", "resource")
	if variant != "resource" && variant != "branch" {
		return ui.Element{}, hostInvalid("invalid resource tree variant")
	}
	binding.state.mu.Lock()
	state := binding.state.trees[identity]
	if state == nil {
		state = &treeDraft{expanded: map[string]*bool{}}
		binding.state.trees[identity] = state
	}
	if !reflect.DeepEqual(state.items, items) {
		state.items = items
		state.expanded = map[string]*bool{}
	}
	state.cursor = current
	binding.state.mu.Unlock()
	emit := func(itemID, kind string, extra map[string]any) {
		event := map[string]any{"tree": identity, "id": itemID, "kind": kind}
		for key, value := range extra {
			event[key] = value
		}
		binding.emit("tree", event)
	}
	externalFiles := nativeProp(props, "external_files", false)
	palette := theme.Of(view)
	root := ui.Scroll(view).Fill().MinWidth(0)
	root.ContextMenu(func(menu *ui.Menu) {
		nativeTreeMenu(menu, nativeProp(props, "menu", []nativeMenuItem{}), func(entry nativeMenuItem) {
			emit("", "menu", nativeTreeAction(entry))
		})
	})
	if externalFiles {
		binding.publishDroppedFiles(identity, "", root.DroppedFiles())
	}
	root.Children(func() {
		tree := ui.TreeBase(view)
		tree.Root.Gap(1).FillWidth().Children(func() {
			var render func([]nativeTreeItem, []bool)
			render = func(items []nativeTreeItem, ancestors []bool) {
				for index, item := range items {
					decoration := nativeTreeDecoration{}
					if item.Decoration != nil {
						decoration = *item.Decoration
					}
					var open *bool
					if len(item.Children) > 0 {
						open = state.expanded[item.ID]
						if open == nil {
							value := item.Expanded
							open = &value
							state.expanded[item.ID] = open
						}
					}
					before := open != nil && *open
					parts := tree.Item(open)
					row := parts.Row.Key(item.ID).Label(item.Label).Height(28).Gap(8).PaddingX(8).MinWidth(0).Radius(palette.Radius).Disabled(item.Disabled).Tooltip(item.Label)
					depth := parts.Depth()
					indent := nativeProp(props, "indent", float32(16))
					skip := max(0, nativeProp(props, "skip_depth", 0))
					left := 8 + indent*float32(max(0, depth-skip))
					active := slices.Contains(selected, item.ID) && !decoration.Group
					if variant == "branch" {
						left = 8 + 20*float32(depth)
						active = state.cursor == item.ID
					}
					row.PaddingStart(left).AccessSelected(active)
					if active {
						row.Background(palette.Active)
					} else if row.Hovered() {
						row.Background(palette.Hover)
					}
					path := append(append([]bool{}, ancestors...), index+1 < len(items))
					if variant == "branch" && depth > 0 {
						row.Draw(func(painter *ui.Painter, bounds ui.Rect) {
							middle := bounds.Y + bounds.H/2
							horizontal := bounds.X + left - 14
							end := bounds.Y + bounds.H
							if index+1 == len(items) {
								end = middle
							}
							nativeTreeDashedLine(painter, horizontal, bounds.Y, horizontal, end, bounds.H/14, palette.MutedForeground.Alpha(0.65))
							nativeTreeDashedLine(painter, horizontal, middle, horizontal+8, middle, bounds.H/14, palette.MutedForeground.Alpha(0.65))
						})
					} else if nativeProp(props, "guides", false) {
						row.Draw(func(painter *ui.Painter, bounds ui.Rect) {
							visible := max(0, len(path)-1-skip)
							for column := 0; column < visible; column++ {
								last := column+1 == visible
								continued := path[skip+column+1]
								if !last && !continued {
									continue
								}
								horizontal := bounds.X + 10.5 + indent*float32(column)
								middle := bounds.Y + bounds.H/2
								end := bounds.Y + bounds.H
								if last && !continued {
									end = middle
								}
								nativeTreeDashedLine(painter, horizontal, bounds.Y, horizontal, end, bounds.H/14, palette.Border)
								if last {
									nativeTreeDashedLine(painter, horizontal, middle, horizontal+indent-4, middle, bounds.H/14, palette.Border)
								}
							}
						})
					}
					row.Children(func() {
						if variant == "branch" && open != nil {
							glyph := icons.ChevronRight
							if *open {
								glyph = icons.ChevronDown
							}
							ui.Icon(view, glyph).Size(12, 12)
						}
						if variant == "resource" {
							binding.renderTreeContent(view, item, decoration, open, emit)
						} else {
							nativeText(view, item.Label).SingleLine().Grow(1).MinWidth(0)
						}
					})
					if row.Clicked() {
						modifiers := row.ClickModifiers()
						state.cursor = item.ID
						emit(item.ID, "select", map[string]any{"shift": modifiers&ui.Shift != 0, "additive": modifiers&(ui.Cmd|ui.Ctrl) != 0})
						if modifiers&(ui.Shift|ui.Cmd|ui.Ctrl) == 0 {
							if open != nil {
								*open = !*open
							} else {
								emit(item.ID, "open", nil)
							}
						}
					} else if row.Focused() && state.cursor != item.ID {
						state.cursor = item.ID
						modifiers := view.Modifiers()
						emit(item.ID, "select", map[string]any{"shift": modifiers&ui.Shift != 0, "additive": modifiers&(ui.Cmd|ui.Ctrl) != 0})
					}
					row.ObserveInput(func(event ui.InputEvent) {
						if event.Kind == ui.InputPointerDown && event.Button == 1 && !item.Disabled {
							emit(item.ID, "context", nil)
						}
					})
					row.ContextMenu(func(menu *ui.Menu) {
						nativeTreeMenu(menu, item.Menu, func(entry nativeMenuItem) {
							emit(item.ID, "menu", nativeTreeAction(entry))
						})
					})
					if !item.Disabled {
						row.Drag(nativeTreeDrag{owner: binding.state, tree: identity, item: item.ID})
						if drag, dropped := ui.Drop[nativeTreeDrag](row); dropped && drag.owner == binding.state && drag.tree == identity && drag.item != item.ID {
							emit(item.ID, "drop", map[string]any{"source": drag.item})
						}
						if externalFiles {
							binding.publishDroppedFiles(identity, item.ID, row.DroppedFiles())
						}
					}
					if open != nil && before != *open {
						kind := "collapse"
						if *open {
							kind = "expand"
						}
						emit(item.ID, kind, nil)
					}
					for _, shortcut := range []struct {
						modifiers ui.Modifiers
						key       ui.Key
						action    string
					}{{ui.Cmd, ui.KeyA, "select_all"}, {ui.Cmd, ui.KeyC, "copy"}, {ui.Cmd, ui.KeyX, "cut"}, {ui.Cmd, ui.KeyV, "paste"}, {0, ui.KeyF2, "rename"}} {
						if row.Shortcut(shortcut.modifiers, shortcut.key) {
							emit(item.ID, "command", map[string]any{"action": shortcut.action})
						}
					}
					parts.Group(func() { render(item.Children, path) })
				}
			}
			render(items, nil)
		})
	})
	return root, nil
}

func nativeTreeAction(item nativeMenuItem) map[string]any {
	value := map[string]any{"action": item.ID}
	if len(item.Value) != 0 && string(item.Value) != "null" {
		value["value"] = item.Value
	}
	return value
}

func nativeTreeMenu(menu *ui.Menu, items []nativeMenuItem, choose func(nativeMenuItem)) {
	for _, item := range items {
		if item.Separator {
			menu.Separator()
			continue
		}
		if menu.Item(item.Label).Disabled(item.Disabled || item.Enabled != nil && !*item.Enabled).Chosen() {
			choose(item)
		}
	}
}

func nativeTreeDashedLine(painter *ui.Painter, startX, startY, endX, endY, dash float32, color ui.Color) {
	if dash <= 0 {
		return
	}
	if startX == endX {
		for position := startY; position < endY; position += dash * 2 {
			painter.Line(startX, position, endX, min(position+dash, endY), 1, color)
		}
	} else {
		for position := startX; position < endX; position += dash * 2 {
			painter.Line(position, startY, min(position+dash, endX), endY, 1, color)
		}
	}
}

func (binding *NativeBinding) renderTreeContent(view *ui.Context, item nativeTreeItem, decoration nativeTreeDecoration, open *bool, emit func(string, string, map[string]any)) {
	palette := theme.Of(view)
	if !decoration.Group {
		glyph := item.Icon
		if glyph == "" {
			glyph = "file"
			if open != nil {
				glyph = "folder"
			}
		}
		size := decoration.IconSize
		if size == 0 {
			size = 16
		}
		if icon := nativeIcon(glyph); icon != nil {
			ui.Icon(view, icon).Size(size, size).TextColor(nativeTone(view, decoration.IconTone))
		}
	}
	ui.Row(view).Grow(1).MinWidth(0).Gap(8).Children(func() {
		nativeText(view, item.Label).Grow(1).MinWidth(0).SingleLine()
		if decoration.Detail != "" {
			nativeText(view, decoration.Detail).TextColor(palette.MutedForeground).SingleLine().Shrink(1)
		}
	})
	if decoration.BadgesWidth != nil || len(decoration.Badges) > 0 {
		badges := ui.Row(view).Gap(8).Justify(ui.End).Shrink(0)
		if decoration.BadgesWidth != nil {
			badges.Width(*decoration.BadgesWidth)
		}
		badges.Children(func() {
			for _, badge := range decoration.Badges {
				nativeText(view, badge.Text).FontSize(12).TextColor(nativeTone(view, badge.Tone))
			}
		})
	}
	if decoration.Group {
		glyph := icons.ChevronRight
		if open != nil && *open {
			glyph = icons.ChevronDown
		}
		ui.Icon(view, glyph).Size(12, 12)
	}
	if check := decoration.Check; check != nil {
		state := ui.CheckOff
		if check.Checked {
			state = ui.CheckOn
		}
		if check.Mixed {
			state = ui.CheckMixed
		}
		if ui.CheckboxState(view.Key(check.ID), &state, "").Label(check.Label).Disabled(check.Disabled).Changed() {
			emit(item.ID, "check", nil)
		}
	}
}
