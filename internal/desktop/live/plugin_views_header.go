package live

import (
	"bytes"
	"encoding/json"
	"errors"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginHeaderContent struct {
	Title            *string              `json:"title"`
	Tabs             *pluginHeaderTabs    `json:"tabs"`
	MinContentWidth  float32              `json:"min_content_width"`
	MinContentHeight float32              `json:"min_content_height"`
	Items            []pluginHeaderItem   `json:"items"`
	Actions          []pluginHeaderAction `json:"actions"`
	Filters          []pluginHeaderFilter `json:"filters"`
	FiltersLabel     string               `json:"filters_label"`
}

type pluginHeaderTabs struct {
	ID         string            `json:"id"`
	Items      []json.RawMessage `json:"items"`
	Selected   *string           `json:"selected"`
	MaxWidth   *float32          `json:"max_width"`
	CloseLabel string            `json:"close_label"`
}

type pluginHeaderItem struct {
	Label string `json:"label"`
	Value string `json:"value"`
	Tone  string `json:"tone"`
}

type pluginHeaderAction struct {
	ID       string                 `json:"id"`
	Label    string                 `json:"label"`
	Icon     string                 `json:"icon"`
	Disabled bool                   `json:"disabled"`
	Menu     []pluginHeaderMenuItem `json:"menu"`
}

type pluginHeaderMenuItem struct {
	ID        string                 `json:"id"`
	Label     string                 `json:"label"`
	Icon      string                 `json:"icon"`
	Separator bool                   `json:"separator"`
	Header    bool                   `json:"header"`
	Enabled   bool                   `json:"enabled"`
	Checked   bool                   `json:"checked"`
	Children  []pluginHeaderMenuItem `json:"children"`
	Shortcut  string                 `json:"shortcut"`
	Value     json.RawMessage        `json:"value"`
}

type pluginHeaderFilter struct {
	ID         string               `json:"id"`
	Label      string               `json:"label"`
	Items      []pluginHeaderChoice `json:"items"`
	Selected   string               `json:"selected"`
	Disabled   bool                 `json:"disabled"`
	Primary    bool                 `json:"primary"`
	Searchable bool                 `json:"searchable"`
	Segmented  bool                 `json:"segmented"`
}

type pluginHeaderChoice struct {
	ID    string `json:"id"`
	Label string `json:"label"`
}

type pluginHeaderFilterState struct {
	open     bool
	query    string
	value    string
	selected int
}

func decodePluginHeader(body json.RawMessage) (pluginHeaderContent, error) {
	var content pluginHeaderContent
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&content); err != nil {
		return content, err
	}
	if len(content.Items) > 4 || len(content.Actions) > 3 || len(content.Filters) > 8 ||
		math.IsNaN(float64(content.MinContentWidth)) || math.IsInf(float64(content.MinContentWidth), 0) || content.MinContentWidth < 0 ||
		math.IsNaN(float64(content.MinContentHeight)) || math.IsInf(float64(content.MinContentHeight), 0) || content.MinContentHeight < 0 {
		return content, errors.New("invalid plugin header content")
	}
	for _, item := range content.Items {
		if item.Tone != "" && item.Tone != "default" && item.Tone != "success" && item.Tone != "danger" {
			return content, errors.New("invalid plugin header item tone")
		}
	}
	for _, action := range content.Actions {
		if len(action.Menu) > 256 || !validPluginHeaderMenu(action.Menu) {
			return content, errors.New("invalid plugin header menu")
		}
	}
	for _, filter := range content.Filters {
		if filter.ID == "" || len(filter.ID) > 128 || len(filter.Label) > 1024 || len(filter.Items) > 1024 {
			return content, errors.New("invalid plugin header filter")
		}
		for _, item := range filter.Items {
			if len(item.ID) > 512 || len(item.Label) > 4096 {
				return content, errors.New("invalid plugin header filter choice")
			}
		}
	}
	return content, nil
}

func validPluginHeaderMenu(items []pluginHeaderMenuItem) bool {
	for _, item := range items {
		if item.Separator {
			continue
		}
		if strings.TrimSpace(item.Label) == "" {
			return false
		}
		if item.Header {
			continue
		}
		if len(item.Children) > 0 {
			if !validPluginHeaderMenu(item.Children) {
				return false
			}
		} else if item.ID == "" || len(item.ID) > 256 {
			return false
		}
	}
	return true
}

func (r *Runtime) activePluginHeader() (*pluginMountedView, pluginHeaderContent) {
	root := r.localRuntime()
	if root.pluginViews == nil {
		return nil, pluginHeaderContent{}
	}
	var key pluginMountKey
	switch root.App.Page {
	case shell.PluginPage:
		entry, exists := root.pluginViews.navigation[root.App.PluginFeature()]
		if !exists {
			return nil, pluginHeaderContent{}
		}
		key = pluginMountKey{scope: pluginScopeIdentity(entry.owner.controller.Target(), entry.scope), role: "navigation", name: entry.scope.Package.Name}
	case shell.SettingsPage:
		entry, exists := root.pluginViews.settings[root.App.Settings.Section]
		if !exists {
			return nil, pluginHeaderContent{}
		}
		key = pluginMountKey{scope: pluginScopeIdentity(entry.owner.controller.Target(), entry.scope), role: "settings", name: entry.scope.Package.Name}
	default:
		return nil, pluginHeaderContent{}
	}
	mounted := root.pluginViews.mounts[key]
	if mounted == nil || mounted.host == nil || mounted.mounted == nil || !root.pluginViewCurrent(mounted) {
		return nil, pluginHeaderContent{}
	}
	native := mounted.host.Native()
	body := native.HeaderSnapshot()
	if len(body) == 0 {
		return nil, pluginHeaderContent{}
	}
	content, err := decodePluginHeader(body)
	if err != nil {
		return nil, pluginHeaderContent{}
	}
	return mounted, content
}

func (r *Runtime) renderPluginTitle(view *ui.Context) bool {
	mounted, content := r.activePluginHeader()
	if mounted == nil {
		return false
	}
	if content.Tabs != nil && len(content.Tabs.Items) > 0 {
		props, err := json.Marshal([]any{content.Tabs.ID, content.Tabs})
		if err != nil {
			return false
		}
		for _, component := range mounted.components {
			if component.Module == "sailry/ui" && component.Export == "NavigationTabs" {
				element, err := component.Render(view.Key("plugin-title").Key(mounted.generation), desktopplugins.NativeElement{
					Module: component.Module, Export: component.Export, Props: props,
				}, func() {}, func(desktopplugins.CallbackRef, json.RawMessage) {})
				if err != nil {
					r.failPluginView(view, mounted, err, "plugin_callback_failed")
				} else if element != (ui.Element{}) && mounted.disconnected {
					element.Disabled(true).DimDisabled(false)
				}
				return err == nil
			}
		}
	}
	if content.Title == nil {
		return false
	}
	ui.Text(view.Key("plugin-title"), *content.Title).FontSize(14).SingleLine().Shrink(1).TextColor(theme.Of(view).Foreground)
	return true
}

func (r *Runtime) emitPluginHeader(mounted *pluginMountedView, identity string) {
	if mounted.host == nil || mounted.ctx.Err() != nil || !r.pluginViewCurrent(mounted) {
		return
	}
	native := mounted.host.Native()
	native.EmitHeaderAction(identity)
}

func (r *Runtime) renderPluginHeader(view *ui.Context) {
	root := r.localRuntime()
	if root.App.Page == shell.ConversationPage {
		if conversation := root.App.Data.Find(root.App.Sidebar.Selected); conversation != nil && root.App.Demo() {
			// A demo conversation is no session of a Node: the statistics
			// package alone is mounted over it (plugin_demo.go).
			if registry := root.demoPluginRegistry(conversation); registry != nil {
				root.renderPluginControls(view, registry, protocol.PluginUiSlotHeader, composer.ControlStart)
			}
		} else if conversation != nil {
			draft := root.App.Composer(conversation.ID)
			if owner := root.conversationRuntime(conversation, draft); owner != nil {
				if registry := owner.pluginRegistry(composerPluginScope(owner, conversation, draft), conversation.ID); registry != nil {
					owner.renderPluginControls(view, registry, protocol.PluginUiSlotHeader, composer.ControlStart)
				}
			}
		}
	}
	mounted, content := r.activePluginHeader()
	if mounted == nil || len(content.Items) == 0 && len(content.Actions) == 0 && len(content.Filters) == 0 {
		return
	}
	current := view.Key("plugin-header").Key(mounted.generation)
	palette := theme.Of(current)
	row := ui.Row(current).MinWidth(0).Shrink(1).Gap(12).AlignItems(ui.Center).Disabled(mounted.disconnected).DimDisabled(false)
	compact := row.Bounds().W > 0 && row.Bounds().W < 660
	row.Children(func() {
		if len(content.Filters) != 0 {
			ui.Row(current.Key("filters")).MinWidth(0).Shrink(1).Gap(4).Children(func() {
				overflow := false
				for _, filter := range content.Filters {
					if compact && !filter.Primary {
						overflow = true
						continue
					}
					r.renderPluginHeaderFilter(current, mounted, filter)
				}
				if overflow {
					button := kit.IconButton(current.Key("overflow"), icons.Settings2, content.FiltersLabel, shell.HeaderButton, false)
					button.OnClick(func() { mounted.headerOverflow = !mounted.headerOverflow })
					kit.Popover(current, button, &mounted.headerOverflow, func() {
						ui.Column(current).Width(240).Gap(4).Padding(8).Children(func() {
							for _, filter := range content.Filters {
								if !filter.Primary {
									r.renderPluginHeaderFilter(current, mounted, filter)
								}
							}
						})
					})
				}
			})
		}
		for index, item := range content.Items {
			ui.Row(current.Key(index)).Gap(8).Children(func() {
				ui.Text(current, item.Label).FontSize(14).TextColor(palette.MutedForeground)
				color := palette.Foreground
				switch item.Tone {
				case "success":
					color = palette.Success
				case "danger":
					color = palette.Danger
				}
				ui.Text(current, item.Value).FontSize(14).FontWeight(600).TextColor(color)
			})
		}
		if len(content.Actions) == 0 {
			return
		}
		// The page's actions are header buttons like the native ones: the
		// same Kit icon button, size and gap.
		ui.Row(current.Key("actions")).Shrink(0).Gap(shell.ButtonGap).AlignItems(ui.Center).Children(func() {
			for _, action := range content.Actions {
				icon, _ := icons.Plugin(action.Icon)
				if action.Menu != nil {
					kit.MenuButton(current.Key(action.ID), action.ID, func(button ui.Element) {
						kit.IconLook(current, button.Disabled(action.Disabled), icon, action.Label, "", shell.HeaderButton, false)
					}, func(menu *kit.Menu) { r.pluginHeaderMenu(menu, mounted, action.Menu) })
				} else {
					kit.IconButton(current.Key(action.ID), icon, action.Label, shell.HeaderButton, false).Disabled(action.Disabled).
						OnClick(func() { r.emitPluginHeader(mounted, action.ID) })
				}
			}
		})
	})
}

func (r *Runtime) pluginHeaderMenu(menu *kit.Menu, mounted *pluginMountedView, items []pluginHeaderMenuItem) {
	for _, item := range items {
		if item.Separator {
			menu.Separator()
			continue
		}
		if item.Header {
			menu.Item(item.Label).Disabled(true)
			continue
		}
		if len(item.Children) > 0 {
			menu.Submenu(item.Label, func(sub *kit.Menu) { r.pluginHeaderMenu(sub, mounted, item.Children) }).Disabled(!item.Enabled)
			continue
		}
		entry := menu.Item(item.Label).Disabled(!item.Enabled).Checked(item.Checked).Shortcut(item.Shortcut)
		if icon, err := icons.Plugin(item.Icon); err == nil {
			entry.Icon(icon)
		}
		if entry.Chosen() {
			r.emitPluginHeader(mounted, item.ID)
		}
	}
}

func (r *Runtime) renderPluginHeaderFilter(view *ui.Context, mounted *pluginMountedView, filter pluginHeaderFilter) {
	if mounted.headerFilters == nil {
		mounted.headerFilters = map[string]*pluginHeaderFilterState{}
	}
	state := mounted.headerFilters[filter.ID]
	if state == nil {
		state = &pluginHeaderFilterState{}
		mounted.headerFilters[filter.ID] = state
	}
	current := view.Key(filter.ID)
	labels, values := make([]string, 0, len(filter.Items)), make([]string, 0, len(filter.Items))
	state.value, state.selected = filter.Selected, -1
	for index, item := range filter.Items {
		labels, values = append(labels, item.Label), append(values, item.ID)
		if item.ID == filter.Selected {
			state.selected = index
		}
	}
	emit := func(value string) {
		body, _ := json.Marshal(map[string]string{"id": filter.ID, "value": value})
		r.emitPluginHeader(mounted, string(body))
	}
	if filter.Segmented {
		if kit.Segmented(current, "segments", &state.selected, labels, kit.SegmentedProps{Size: kit.SizeSmall}).
			Label(filter.Label).Disabled(filter.Disabled).Changed() && state.selected >= 0 && state.selected < len(values) {
			emit(values[state.selected])
		}
		return
	}
	if !filter.Searchable {
		control, changed := kit.Select(current, "select", filter.Label, &state.value, labels, filter.Disabled, values)
		control.Width(112).MinWidth(0)
		if changed {
			emit(state.value)
		}
		return
	}
	label := filter.Selected
	if state.selected >= 0 {
		label = labels[state.selected]
	}
	// The searchable filter is the same control as the plain one: Kit's
	// select trigger over a popup that adds the query.
	button := kit.SelectTrigger(current, ui.ButtonBase(current).Label(filter.Label).Disabled(filter.Disabled), label, false, state.open).
		Width(112).MinWidth(0).OnClick(func() { state.open, state.query = !state.open, "" })
	kit.Popover(current, button, &state.open, func() {
		ui.Column(current).Width(240).Gap(4).Padding(4).Children(func() {
			ui.TextInput(current.Key("query"), &state.query).Label(filter.Label).FillWidth()
			ui.Scroll(current).MaxHeight(256).FillWidth().Children(func() {
				for _, item := range filter.Items {
					if state.query != "" && !strings.Contains(strings.ToLower(item.Label), strings.ToLower(state.query)) {
						continue
					}
					kit.Ghost(current.Key(item.ID), item.Label, item.ID == filter.Selected).FillWidth().Justify(ui.Start).Padding(6, 10).
						Children(func() { ui.Text(current, item.Label).SingleLine() }).
						OnClick(func() { state.open = false; emit(item.ID) })
				}
			})
		})
	})
}
