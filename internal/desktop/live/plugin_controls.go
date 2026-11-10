package live

import (
	"encoding/json"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginControlPresentation struct {
	key      plugins.ContributionKey
	open     bool
	query    string
	previous string
	selected int
}

func pluginLabel(value protocol.PluginDesktopNavigation, locale string) string {
	if translated := value.Locales[i18n.Match(locale)]; translated != "" {
		return translated
	}
	return value.Label
}

func pluginControlIdentity(key plugins.ContributionKey) string {
	body, _ := json.Marshal(key)
	return string(body)
}

func composerPluginScope(r *Runtime, cv *viewmodel.Conversation, comp *composer.State) protocol.PluginContext {
	scope := protocol.DefaultPluginContext()
	scope.Surface.Kind = "composer"
	if session, exists := r.session(cv.ID); exists {
		scope.Worktree, scope.Session = &session.Worktree, &session.ID
		return scope
	}
	for _, tree := range r.controller.Snapshot().Worktrees {
		if tree.Path == comp.Worktree && (tree.Project == nil && comp.Project == "" || tree.Project != nil && tree.Project.String() == comp.Project) {
			scope.Worktree = &tree.ID
			return scope
		}
	}
	if r.workspace != nil {
		for _, tree := range r.workspace.admitted {
			if tree.Path == comp.Worktree && (tree.Project == nil && comp.Project == "" || tree.Project != nil && tree.Project.String() == comp.Project) {
				scope.Worktree = &tree.ID
				break
			}
		}
	}
	return scope
}

func (r *Runtime) projectPluginComposer(view *ui.Context, cv *viewmodel.Conversation, comp *composer.State, context *composer.Context) {
	scope := composerPluginScope(r, cv, comp)
	registry := r.pluginRegistry(scope, cv.ID)
	comp.SetPluginControls(func(view *ui.Context, slot string) {
		if registry != nil {
			r.renderPluginControls(view, registry, protocol.PluginUiSlot(slot))
		}
	})
	r.projectPluginStatus(comp, context, scope, registry)
	if registry == nil {
		comp.SetPluginCommand(nil)
		return
	}
	commands := map[string]plugins.ContributionEntry{}
	for _, entry := range registry.Commands() {
		if entry.Declaration.Command == nil {
			continue
		}
		id := pluginControlIdentity(entry.Key)
		commands[id] = entry
		context.Commands = append(context.Commands, composer.Reference{
			ID: id, Source: "plugin-contribution", Kind: composer.Prompt,
			Label: "/" + entry.Declaration.Command.Name, Description: pluginLabel(entry.Declaration.Label, view.Locale()),
			Icon: pluginEntryIcon(entry), Group: "reference_group_plugins",
		})
	}
	comp.SetPluginCommand(func(reference composer.Reference) (string, bool) {
		if reference.Source != "plugin-contribution" {
			return "", false
		}
		entry, exists := commands[reference.ID]
		if !exists || entry.Declaration.Command == nil {
			return "", true
		}
		if entry.Declaration.Command.Kind == "message" {
			return "/" + entry.Declaration.Command.Name + " ", true
		}
		r.invokePluginControl(registry, entry.Key, "invoke", nil)
		return "", true
	})
}

func pluginEntryIcon(entry plugins.ContributionEntry) *ui.SVG {
	name := entry.Declaration.Icon
	if entry.State.Icon != nil {
		name = entry.State.Icon
	}
	if name != nil {
		if icon, err := icons.Plugin(*name); err == nil {
			return icon
		}
	}
	return icons.Settings2
}

func pluginEntryChoices(entry plugins.ContributionEntry) []protocol.PluginUiChoice {
	if entry.State.Choices != nil {
		return *entry.State.Choices
	}
	return entry.Declaration.Choices
}

func pluginEntryLabel(entry plugins.ContributionEntry, locale string) string {
	var value string
	_ = json.Unmarshal(entry.State.Value, &value)
	if entry.Declaration.Kind != "menu" || entry.State.Label == nil {
		var find func([]protocol.PluginUiChoice) string
		find = func(choices []protocol.PluginUiChoice) string {
			for _, choice := range choices {
				if choice.ID == value && len(choice.Children) == 0 {
					return pluginLabel(choice.Label, locale)
				}
				if label := find(choice.Children); label != "" {
					return label
				}
			}
			return ""
		}
		if label := find(pluginEntryChoices(entry)); label != "" {
			return label
		}
	}
	if entry.State.Label != nil {
		return pluginLabel(*entry.State.Label, locale)
	}
	return pluginLabel(entry.Declaration.Label, locale)
}

func (r *Runtime) invokePluginControl(registry *plugins.Registry, key plugins.ContributionKey, kind protocol.PluginUiEventKind, value any) {
	body, err := json.Marshal(value)
	if err == nil {
		err = registry.Invoke(key, kind, body)
	}
	if err != nil {
		r.Report(err)
	}
	r.localRuntime().wake()
}

func (r *Runtime) renderPluginControls(view *ui.Context, registry *plugins.Registry, slot protocol.PluginUiSlot) {
	for _, entry := range registry.Entries(slot) {
		r.renderPluginControl(view.Key(pluginControlIdentity(entry.Key)), registry, entry)
	}
}

func (r *Runtime) renderPluginControl(view *ui.Context, registry *plugins.Registry, entry plugins.ContributionEntry) {
	identity := pluginControlIdentity(entry.Key)
	if r.pluginControls == nil {
		r.pluginControls = map[string]*pluginControlPresentation{}
	}
	state := r.pluginControls[identity]
	if state == nil {
		state = &pluginControlPresentation{key: entry.Key}
		r.pluginControls[identity] = state
	}
	label, icon := pluginEntryLabel(entry, view.Locale()), pluginEntryIcon(entry)
	palette := theme.Of(view)
	selected := false
	_ = json.Unmarshal(entry.State.Value, &selected)
	if entry.Retry {
		kit.Ghost(view, label, false).Disabled(true)
		kit.IconButton(view.Key(identity+"/retry"), icons.RotateCw, i18n.T(view, "plugins_retry"), 28, false).
			OnClick(func() { r.retryPluginContribution(entry.Key) })
		return
	}
	var indicator protocol.PluginUiIndicator
	if entry.Declaration.Kind == "indicator" {
		_ = json.Unmarshal(entry.State.Value, &indicator)
		selected = indicator.Open
		label = indicator.Hint
	}
	look := func(button ui.Element) {
		kit.GhostLook(view, button, selected).Label(label).Disabled(!entry.State.Enabled).
			Height(32).PaddingX(12).Gap(6).Radius(16).TextColor(palette.Foreground).
			Children(func() {
				if entry.State.Loading {
					ui.Spinner(view).Size(16, 16)
				} else if entry.Declaration.Kind == "indicator" {
					pluginIndicator(view, indicator)
				} else {
					ui.Icon(view, icon).FontSize(16)
				}
				if entry.Declaration.Slot != "model" && entry.Declaration.Slot != "header" && entry.Declaration.Kind != "indicator" {
					ui.Text(view, label).FontSize(14).SingleLine()
				}
			})
		if entry.Declaration.Slot == "model" || entry.Declaration.Slot == "header" || entry.Declaration.Kind == "indicator" {
			kit.Tooltip(view, button, label)
		}
	}
	switch entry.Declaration.Kind {
	case "select", "menu":
		kit.MenuButton(view, identity, look, func(menu *kit.Menu) {
			r.pluginChoiceMenu(view, menu, registry, entry, pluginEntryChoices(entry), entry.State.Enabled)
		}, kit.MenuOptions{At: ui.AnchorTopStart, Self: ui.AnchorBottomStart, MaxHeight: 360})
	case "picker":
		button := ui.ButtonBase(view)
		look(button)
		button.OnClick(func() {
			state.open, state.query, state.previous, state.selected = true, "", "", 0
			r.invokePluginControl(registry, entry.Key, "search", "")
		})
		r.pluginChoicePicker(view, registry, entry, state)
	case "popover", "indicator":
		button := ui.ButtonBase(view)
		look(button)
		if entry.Declaration.Handler == nil {
			return
		}
		state.open = selected
		button.HasPopup(ui.PopupDialog).OnClick(func() {
			r.invokePluginControl(registry, entry.Key, "change", !state.open)
		})
		wasOpen := state.open
		panel := kit.Popover(view, button, &state.open, func() {
			r.renderPluginContribution(view, entry.Key)
		})
		if panel != (ui.Element{}) {
			panel.AttachTo(button, ui.AnchorTopStart, ui.AnchorBottomStart).Width(320).Padding(12)
		}
		if wasOpen && !state.open {
			r.invokePluginControl(registry, entry.Key, "change", false)
		}
	case "metric":
		value := string(entry.State.Value)
		if value == "null" || value == "" {
			value = i18n.T(view, "composer_metric_unknown")
		} else {
			var text string
			if json.Unmarshal(entry.State.Value, &text) == nil {
				value = text
			}
		}
		kit.Tooltip(view, ui.Text(view, value).FontSize(12).TextColor(palette.MutedForeground), label)
	default:
		button := ui.ButtonBase(view)
		look(button)
		button.OnClick(func() {
			if entry.Declaration.Kind == "toggle" {
				r.invokePluginControl(registry, entry.Key, "change", !selected)
			} else {
				r.invokePluginControl(registry, entry.Key, "invoke", nil)
			}
		})
	}
}

func (r *Runtime) pluginControlsAfterFrame() {
	for identity, presentation := range r.pluginControls {
		if owner, _ := r.pluginContribution(presentation.key); owner == nil {
			delete(r.pluginControls, identity)
		}
	}
}

func (r *Runtime) pluginChoiceMenu(view *ui.Context, menu *kit.Menu, registry *plugins.Registry, entry plugins.ContributionEntry, choices []protocol.PluginUiChoice, enabled bool) {
	var selected string
	_ = json.Unmarshal(entry.State.Value, &selected)
	group := ""
	for _, choice := range choices {
		if choice.Group != nil {
			label := pluginLabel(*choice.Group, view.Locale())
			if label != group {
				if group != "" {
					menu.Separator()
				}
				group = label
				menu.Item(label).Disabled(true).Chosen()
			}
		}
		label := pluginLabel(choice.Label, view.Locale())
		if len(choice.Children) > 0 {
			menu.Submenu(label, func(child *kit.Menu) {
				r.pluginChoiceMenu(view, child, registry, entry, choice.Children, enabled && choice.Enabled)
			}).Disabled(!enabled || !choice.Enabled)
			continue
		}
		item := menu.Item(label).Disabled(!enabled || !choice.Enabled).Checked(choice.ID == selected)
		if choice.Description != nil {
			item.Description(pluginLabel(*choice.Description, view.Locale()))
		}
		if choice.Icon != nil {
			if icon, err := icons.Plugin(*choice.Icon); err == nil {
				item.Icon(icon)
			}
		}
		if item.Chosen() {
			r.invokePluginControl(registry, entry.Key, "change", choice.ID)
		}
	}
}

func (r *Runtime) pluginChoicePicker(view *ui.Context, registry *plugins.Registry, entry plugins.ContributionEntry, state *pluginControlPresentation) {
	if !state.open {
		return
	}
	var items []kit.CommandItem
	for _, choice := range pluginEntryChoices(entry) {
		label := pluginLabel(choice.Label, view.Locale())
		if !strings.Contains(strings.ToLower(label), strings.ToLower(state.query)) {
			continue
		}
		item := kit.CommandItem{ID: choice.ID, Label: label, Disabled: !entry.State.Enabled || !choice.Enabled}
		if choice.Description != nil {
			item.Detail = pluginLabel(*choice.Description, view.Locale())
		}
		if choice.Group != nil {
			item.Group = pluginLabel(*choice.Group, view.Locale())
		}
		if choice.Icon != nil {
			item.Icon, _ = icons.Plugin(*choice.Icon)
		}
		items = append(items, item)
	}
	kit.Command(view, "plugin-picker", &state.open, &state.query, &state.selected, items,
		kit.CommandProps{Placeholder: i18n.T(view, "connection_search"), Empty: i18n.T(view, "connection_search_empty")}, func(index int) {
			r.invokePluginControl(registry, entry.Key, "change", items[index].ID)
			state.open = false
		})
	if state.query != state.previous {
		state.previous = state.query
		r.invokePluginControl(registry, entry.Key, "search", state.query)
	}
}

func pluginIndicator(view *ui.Context, indicator protocol.PluginUiIndicator) {
	palette := theme.Of(view)
	color := palette.MutedForeground
	switch indicator.Tone {
	case "warning":
		color = palette.Warning
	case "danger":
		color = palette.Danger
	}
	percent := 0.0
	if indicator.Percent != nil {
		percent = min(max(*indicator.Percent/100, 0), 1)
	}
	ui.Box(view).Size(20, 20).MarginX(4).Shrink(0).Label(indicator.Hint).Draw(func(painter *ui.Painter, bounds ui.Rect) {
		var track ui.Path
		track.Circle(bounds.X+bounds.W/2, bounds.Y+bounds.H/2, bounds.W/2-2)
		painter.StrokePath(&track, 2, color.Alpha(0.25))
		fraction, start := percent, -math.Pi/2
		if indicator.Loading {
			fraction = 0.24
			if !view.Preferences().ReduceMotion {
				start += float64(painter.Now().UnixMilli()%1600) / 1600 * 2 * math.Pi
				painter.AnimationFrameRate(kit.FPS)
			}
		}
		if fraction <= 0 {
			return
		}
		var path ui.Path
		steps := max(2, int(48*fraction))
		for index := 0; index <= steps; index++ {
			angle := start + 2*math.Pi*fraction*float64(index)/float64(steps)
			horizontal := bounds.X + bounds.W/2 + (bounds.W/2-2)*float32(math.Cos(angle))
			vertical := bounds.Y + bounds.H/2 + (bounds.H/2-2)*float32(math.Sin(angle))
			if index == 0 {
				path.MoveTo(horizontal, vertical)
			} else {
				path.LineTo(horizontal, vertical)
			}
		}
		painter.StrokePath(&path, 2, color)
	})
}
