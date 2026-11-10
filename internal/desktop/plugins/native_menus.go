package plugins

import (
	"encoding/json"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type nativeMenuItem struct {
	ID        string           `json:"id"`
	Label     string           `json:"label"`
	Icon      string           `json:"icon"`
	Separator bool             `json:"separator"`
	Header    bool             `json:"header"`
	Enabled   *bool            `json:"enabled"`
	Disabled  bool             `json:"disabled"`
	Checked   bool             `json:"checked"`
	Children  []nativeMenuItem `json:"children"`
	Shortcut  string           `json:"shortcut"`
	Value     json.RawMessage  `json:"value"`
}

func (binding *NativeBinding) menuItems(menu *kit.Menu, items []nativeMenuItem, choose func(nativeMenuItem)) {
	for _, item := range items {
		if item.Separator {
			menu.Separator()
			continue
		}
		if len(item.Children) > 0 {
			menu.Submenu(item.Label, func(sub *kit.Menu) { binding.menuItems(sub, item.Children, choose) }).Disabled(item.Disabled || item.Enabled != nil && !*item.Enabled)
			continue
		}
		entry := menu.Item(item.Label).Disabled(item.Header || item.Disabled || item.Enabled != nil && !*item.Enabled).Checked(item.Checked).Shortcut(item.Shortcut)
		if icon := nativeIcon(item.Icon); icon != nil {
			entry.Icon(icon)
		}
		if entry.Chosen() {
			choose(item)
		}
	}
}

func (binding *NativeBinding) renderSelect(view *ui.Context, identity string, props map[string]json.RawMessage) (ui.Element, error) {
	items := nativeProp(props, "items", []nativeMenuItem{})
	if len(items) > 4096 {
		return ui.Element{}, hostInvalid("plugin selection limit exceeded")
	}
	selected := nativeProp[*string](props, "selected", nil)
	label := nativeProp(props, "label", "")
	title := nativeProp(props, "placeholder", "")
	for _, item := range items {
		if selected != nil && item.ID == *selected {
			title = item.Label
			break
		}
	}
	return kit.MenuButton(view, identity, func(button ui.Element) {
		kit.GhostLook(view, button.Label(label).Disabled(nativeProp(props, "disabled", false)).Padding(6, 10).Gap(8), false).Children(func() {
			nativeText(view, title).SingleLine()
			ui.Icon(view, icons.ChevronDown).Size(12, 12)
		})
	}, func(menu *kit.Menu) {
		for _, item := range items {
			if menu.Item(item.Label).Checked(selected != nil && item.ID == *selected).Disabled(item.Disabled).Chosen() {
				binding.emit("control", map[string]any{"id": identity, "value": item.ID})
			}
		}
	}, kit.MenuOptions{Match: true, CheckRight: true, MaxHeight: 320}), nil
}

func (binding *NativeBinding) renderMenu(view *ui.Context, identity string, props map[string]json.RawMessage) (ui.Element, error) {
	items := nativeProp(props, "items", []nativeMenuItem{})
	if len(items) > 256 {
		return ui.Element{}, hostInvalid("plugin menu limit exceeded")
	}
	label := nativeProp(props, "label", "")
	if strings.TrimSpace(label) == "" {
		return ui.Element{}, hostInvalid("plugin menu requires a label")
	}
	return kit.MenuButton(view, identity, func(button ui.Element) {
		kit.GhostLook(view, button.Label(label).Disabled(nativeProp(props, "disabled", false)).Padding(6, 10).Gap(6), false).Children(func() {
			if icon := nativeIcon(nativeProp(props, "icon", "")); icon != nil {
				ui.Icon(view, icon).Size(16, 16)
			}
			nativeText(view, label)
			if nativeProp(props, "caret", false) {
				ui.Icon(view, icons.ChevronDown).Size(12, 12)
			}
		})
	}, func(menu *kit.Menu) {
		binding.menuItems(menu, items, func(item nativeMenuItem) {
			event := map[string]any{"menu": identity, "id": item.ID}
			if len(item.Value) > 0 {
				event["value"] = item.Value
			}
			binding.emit("menu", event)
		})
	}), nil
}

func (binding *NativeBinding) renderContextMenu(view *ui.Context, identity string, props map[string]json.RawMessage, children func()) (ui.Element, error) {
	items := nativeProp(props, "items", []nativeMenuItem{})
	if len(items) > 256 {
		return ui.Element{}, hostInvalid("plugin context menu limit exceeded")
	}
	region := ui.Column(view).MinWidth(0).MinHeight(0)
	if nativeProp(props, "fill", true) {
		region.Grow(1)
	}
	region.Children(children).ContextMenu(func(menu *ui.Menu) {
		for _, item := range items {
			if item.Separator {
				menu.Separator()
				continue
			}
			enabled := item.Enabled == nil || *item.Enabled
			if menu.Item(item.Label).Disabled(!enabled || item.Disabled).Chosen() {
				event := map[string]any{"menu": identity, "id": item.ID}
				if len(item.Value) > 0 {
					event["value"] = item.Value
				}
				binding.emit("context-menu", event)
			}
		}
	})
	return region, nil
}

func (binding *NativeBinding) renderTabs(view *ui.Context, identity string, props map[string]json.RawMessage, navigation bool) ui.Element {
	items := nativeProp(props, "items", []map[string]json.RawMessage{})
	selected := nativeProp[*string](props, "selected", nil)
	return ui.ScrollHorizontal(view).FillWidth().Children(func() {
		ui.Row(view).Gap(4).Children(func() {
			for _, item := range items {
				itemID := nativeProp(item, "id", "")
				label := nativeProp(item, "label", "")
				row := ui.Row(view.Key(itemID)).Gap(2).MaxWidth(nativeProp(props, "max_width", float32(240)))
				row.Children(func() {
					button := kit.Ghost(view, label, selected != nil && itemID == *selected).Padding(6, 10).Gap(6).MinWidth(0).Disabled(nativeProp(props, "disabled", false))
					button.Children(func() {
						if nativeProp(item, "loading", false) {
							ui.Spinner(view).Size(14, 14)
						} else if icon := nativeIcon(nativeProp(item, "icon", "")); icon != nil {
							ui.Icon(view, icon).Size(14, 14)
						}
						nativeText(view, label).SingleLine()
					}).OnClick(func() {
						if navigation {
							binding.emit("navigation-tabs", map[string]any{"bar": identity, "id": itemID, "kind": "select"})
						} else {
							binding.emit("control", map[string]any{"id": identity, "value": itemID})
						}
					})
					if navigation && nativeProp(item, "closable", false) {
						kit.IconButton(view, icons.Close, nativeProp(item, "close_label", nativeProp(props, "close_label", "")), 20, false).
							Disabled(nativeProp(item, "close_disabled", false)).OnClick(func() {
							binding.emit("navigation-tabs", map[string]any{"bar": identity, "id": itemID, "kind": "close"})
						})
					}
				})
			}
		})
	})
}

func (binding *NativeBinding) renderAppearance(view *ui.Context, identity string, props map[string]json.RawMessage) (ui.Element, error) {
	value := nativeProp(props, "value", map[string]string{})
	colors := []string{"none", "blue", "indigo", "violet", "magenta", "red", "orange", "amber", "green", "teal", "cyan"}
	glyphs := []string{"folder", "code", "terminal", "globe", "server", "database", "package", "game", "book", "finance", "device", "education", "writing", "tags", "music", "media", "design", "health", "nature", "business", "analytics", "profile", "fitness", "law", "audio", "travel", "tools", "science", "ai", "favorite"}
	return kit.MenuButton(view, identity, func(button ui.Element) {
		kit.IconLook(view, button.Disabled(nativeProp(props, "disabled", false)), nativeIcon(value["icon"]), nativeProp(props, "label", ""), "", 28, false)
	}, func(menu *kit.Menu) {
		menu.Submenu(i18n.T(view, "appearance_icon"), func(sub *kit.Menu) {
			for _, glyph := range glyphs {
				if sub.Item(i18n.T(view, "project_icon_"+glyph)).Checked(value["icon"] == glyph).Chosen() {
					binding.emit("control", map[string]any{"id": identity, "value": map[string]string{"icon": glyph, "color": value["color"]}})
				}
			}
		})
		menu.Submenu(i18n.T(view, "appearance_color"), func(sub *kit.Menu) {
			for _, color := range colors {
				if sub.Item(i18n.T(view, "project_color_"+color)).Checked(value["color"] == color).Chosen() {
					binding.emit("control", map[string]any{"id": identity, "value": map[string]string{"icon": value["icon"], "color": color}})
				}
			}
		})
	}), nil
}

func (binding *NativeBinding) renderModelPopup(view *ui.Context, identity string, props map[string]json.RawMessage) (ui.Element, error) {
	catalog := nativeProp(props, "catalog", map[string]json.RawMessage{})
	models := nativeProp(catalog, "models", []map[string]json.RawMessage{})
	selected := nativeProp[*string](props, "selected", nil)
	label := nativeProp(props, "label", "")
	title := nativeProp(props, "placeholder", "")
	for _, model := range models {
		if selected != nil && nativeProp(model, "id", "") == *selected {
			title = nativeProp(model, "name", *selected)
		}
	}
	return kit.MenuButton(view, identity, func(button ui.Element) {
		kit.GhostLook(view, button.Label(label).Disabled(nativeProp(props, "disabled", false)).Padding(6, 10), false).Children(func() { nativeText(view, title) })
	}, func(menu *kit.Menu) {
		for _, model := range models {
			if nativeProp(model, "kind", "") != "provider" {
				continue
			}
			modelID := nativeProp(model, "id", "")
			name := nativeProp(model, "name", modelID)
			efforts := nativeProp(model, "efforts", []json.RawMessage{})
			choose := func(effort any) {
				binding.emit("control", map[string]any{"id": identity, "value": map[string]any{"model": modelID, "effort": effort}})
			}
			if len(efforts) == 0 {
				if menu.Item(name).Checked(selected != nil && *selected == modelID).Chosen() {
					choose(nil)
				}
				continue
			}
			menu.Submenu(name, func(sub *kit.Menu) {
				for _, effort := range efforts {
					text := string(effort)
					_ = json.Unmarshal(effort, &text)
					if sub.Item(text).Chosen() {
						choose(effort)
					}
				}
			})
		}
	}), nil
}
