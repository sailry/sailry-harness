package plugins

import (
	"encoding/json"
	"slices"
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
	// The package owns the selection: the Kit select works on a copy and a
	// new choice is reported, to come back in the next props.
	chosen := ""
	options := make([]kit.SelectOption, len(items))
	for index, item := range items {
		options[index] = kit.SelectOption{Value: item.ID, Label: item.Label, Disabled: item.Disabled}
	}
	placeholder := nativeProp(props, "placeholder", "")
	if selected != nil {
		chosen = *selected
	} else if placeholder == "" {
		// Kit shows the value itself without a placeholder; none is chosen.
		placeholder = " "
	}
	field, changed := kit.SelectField(view, identity, &chosen, kit.SelectProps{
		Label:       nativeProp(props, "label", placeholder),
		Placeholder: placeholder,
		Options:     options,
		Disabled:    nativeProp(props, "disabled", false),
	})
	if changed {
		binding.emit("control", map[string]any{"id": identity, "value": chosen})
	}
	// The reference's SelectField fills the width its package gives it.
	return field.FillWidth(), nil
}

// renderMenuButton is the reference's MenuButton: a Kit button with its
// label and a caret, opening a menu of checked and disabled items. It is not
// a select: the label does not follow the choice.
func (binding *NativeBinding) renderMenuButton(view *ui.Context, identity string, props map[string]json.RawMessage) (ui.Element, error) {
	items := nativeProp(props, "items", []nativeMenuItem{})
	if len(items) > 4096 {
		return ui.Element{}, hostInvalid("plugin selection limit exceeded")
	}
	selected := nativeProp[*string](props, "selected", nil)
	label := nativeProp(props, "label", "")
	disabled := nativeProp(props, "disabled", false)
	return kit.MenuButton(view, identity, func(button ui.Element) {
		kit.SecondaryLook(view, button.Label(label).Disabled(disabled)).Children(func() {
			nativeText(view, label).SingleLine()
			ui.Icon(view, icons.ChevronDown).FontSize(14).Shrink(0)
		})
		if disabled {
			button.Opacity(0.5)
		}
	}, func(menu *kit.Menu) {
		for _, item := range items {
			if menu.Item(item.Label).Checked(selected != nil && item.ID == *selected).Disabled(item.Disabled).Chosen() {
				binding.emit("control", map[string]any{"id": identity, "value": item.ID})
			}
		}
	}), nil
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
	icon := nativeIcon(nativeProp(props, "icon", ""))
	disabled := nativeProp(props, "disabled", false)
	return kit.MenuButton(view, identity, func(button ui.Element) {
		if icon != nil && !nativeProp(props, "caret", false) && !nativeProp(props, "show_label", false) {
			// An icon's menu is Kit's icon button, as the application's own
			// row menus: the label is its tooltip and accessible name.
			size := float32(28)
			if nativeProp(props, "size", "") == "medium" {
				size = 32
			}
			kit.IconLook(view, button.Disabled(disabled), icon, label, "", size, false)
			return
		}
		kit.GhostLook(view, button.Label(label).Disabled(disabled).Padding(6, 10).Gap(6), false).Children(func() {
			if icon != nil {
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
	if !navigation {
		// The reference's SegmentedTabs is Kit's segmented tab bar.
		index, labels, glyphs := -1, make([]string, len(items)), make([]*ui.SVG, len(items))
		for at, item := range items {
			labels[at], glyphs[at] = nativeProp(item, "label", ""), nativeIcon(nativeProp(item, "icon", ""))
			if selected != nil && nativeProp(item, "id", "") == *selected {
				index = at
			}
		}
		before, disabled := index, nativeProp(props, "disabled", false)
		track := kit.Segmented(view, identity, &index, labels, kit.SegmentedProps{Block: nativeProp(props, "fill", false), Icons: glyphs, Disabled: disabled})
		if index != before && !disabled && index >= 0 && index < len(items) {
			binding.emit("control", map[string]any{"id": identity, "value": nativeProp(items[index], "id", "")})
		}
		return track
	}
	return ui.ScrollHorizontal(view).FillWidth().Children(func() {
		ui.Row(view).Gap(4).Children(func() {
			for _, item := range items {
				itemID := nativeProp(item, "id", "")
				label := nativeProp(item, "label", "")
				if navigation {
					// A page's tabs are the right panel's strip tabs.
					active := selected != nil && itemID == *selected
					button, hovered := kit.StripTab(view, itemID, label, active, nativeProp(props, "max_width", float32(240)))
					var close func()
					if nativeProp(item, "closable", false) && !nativeProp(item, "close_disabled", false) {
						close = func() {
							binding.emit("navigation-tabs", map[string]any{"bar": identity, "id": itemID, "kind": "close"})
						}
					}
					kit.StripTabBody(view, button.Disabled(nativeProp(props, "disabled", false)), nativeIcon(nativeProp(item, "icon", "")), label, active, hovered,
						nativeProp(item, "loading", false), nativeProp(item, "close_label", nativeProp(props, "close_label", "")), close).OnClick(func() {
						binding.emit("navigation-tabs", map[string]any{"bar": identity, "id": itemID, "kind": "select"})
					})
					continue
				}
				row := ui.Row(view.Key(itemID)).Gap(2).MaxWidth(nativeProp(props, "max_width", float32(240)))
				row.Children(func() {
					button := kit.Ghost(view, label, selected != nil && itemID == *selected).Padding(6, 10).Gap(6).MinWidth(0).Disabled(nativeProp(props, "disabled", false))
					button.Children(func() {
						if nativeProp(item, "loading", false) {
							kit.Busy(view)
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
	// The catalog's provider models as kit.ModelSelect offers them: under
	// the application's names, marks and scores where it knows them
	// (NativeBinding.Models), else under the catalog's provider and model.
	// An effort is named by its text and sent as the catalog has it.
	var known []kit.ModelOption
	if binding.Models != nil {
		known = binding.Models()
	}
	level := func(effort json.RawMessage) string {
		text := string(effort)
		if json.Unmarshal(effort, &text) != nil {
			// A token budget is named by its number.
			var budget struct {
				Budget *json.Number `json:"budget"`
			}
			if json.Unmarshal(effort, &budget) == nil && budget.Budget != nil {
				text = budget.Budget.String()
			}
		}
		return text
	}
	var offered []kit.ModelOption
	sent := map[string]map[string]json.RawMessage{}
	for _, model := range models {
		if nativeProp(model, "kind", "") != "provider" {
			continue
		}
		id := nativeProp(model, "id", "")
		option := kit.ModelOption{ID: id, Name: nativeProp(model, "name", nativeProp(model, "model", id)), Provider: nativeProp(model, "provider", "")}
		if at := slices.IndexFunc(known, func(named kit.ModelOption) bool { return named.ID == id }); at >= 0 {
			option.Name, option.Mark, option.Score = known[at].Name, known[at].Mark, known[at].Score
		}
		sent[id] = map[string]json.RawMessage{}
		for _, effort := range nativeProp(model, "efforts", []json.RawMessage{}) {
			option.Efforts = append(option.Efforts, level(effort))
			sent[id][level(effort)] = effort
		}
		option.DefaultEffort = level(model["default_effort"])
		offered = append(offered, option)
	}
	selection := kit.ModelSelectProps{
		Label: nativeProp(props, "label", ""), Placeholder: nativeProp(props, "placeholder", ""), Models: offered,
		Effort: level(props["effort"]), Disabled: nativeProp(props, "disabled", false),
		OnChange: func(model, effort string) {
			// A model without efforts sends none, as its menu item did.
			var value any
			if raw, listed := sent[model][effort]; listed {
				value = raw
			}
			binding.emit("control", map[string]any{"id": identity, "value": map[string]any{"model": model, "effort": value}})
		},
	}
	if selected := nativeProp[*string](props, "selected", nil); selected != nil {
		selection.Model = *selected
	}
	return kit.ModelSelect(view.Key(identity), selection).FillWidth(), nil
}
