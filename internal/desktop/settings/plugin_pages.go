package settings

import (
	"cmp"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type PluginPage struct {
	Section Section
	Label   string
	Icon    *ui.SVG
	Group   string
	Order   uint16
	Render  func(*ui.Context)
}

func (section Section) navigationOrder() uint16 {
	switch section {
	case General, Providers, Terminal, Permissions:
		return 100
	case Connectors:
		return 150
	case Teams:
		return 155
	case Plugins:
		return 160
	case Skills:
		return 170
	case Knowledge, Media, Appearance, Connections:
		return 200
	case Backup, Decision, Conversation:
		return 300
	case Shortcuts:
		return 400
	case Hooks, Dictation:
		return 500
	case Runtime:
		return 600
	case OCR:
		return 700
	case Experimental:
		// After every page of its group but About, which ends the navigation.
		return 65000
	case About:
		return 65535
	default:
		return 10000
	}
}

func settingsGroupNavigation(view *ui.Context, group string, core []Item, pages []PluginPage) []PluginPage {
	entries := make([]PluginPage, 0, len(core)+len(pages))
	for _, item := range core {
		entries = append(entries, PluginPage{
			Section: item.Section, Label: i18n.T(view, string(item.Section)), Icon: item.Icon,
			Order: item.Section.navigationOrder(),
		})
	}
	for _, page := range pages {
		if page.Group == group {
			entries = append(entries, page)
		}
	}
	slices.SortStableFunc(entries, func(first, second PluginPage) int { return cmp.Compare(first.Order, second.Order) })
	return entries
}

func (state *State) SetPluginPages(pages []PluginPage) {
	previous := slices.ContainsFunc(state.pluginPages, func(page PluginPage) bool { return page.Section == state.Section })
	state.pluginPages = slices.Clone(pages)
	if previous && !slices.ContainsFunc(pages, func(page PluginPage) bool { return page.Section == state.Section }) {
		state.Open(Plugins)
	}
}

func (state *State) PluginPages() []PluginPage {
	return slices.Clone(state.pluginPages)
}

func settingsNavigation(view *ui.Context, state *State, section Section, label string, icon *ui.SVG) {
	palette := theme.Of(view)
	selected := section == state.Section
	button := ui.ButtonBase(view.Key(string(section))).Label(label).Height(28).PaddingX(8).Gap(8).Radius(palette.Radius).
		Justify(ui.Start).TextColor(palette.SidebarForeground)
	button.Background(palette.SidebarItem(selected, button.Hovered())).AccessSelected(selected)
	if selected {
		button.TextColor(palette.SidebarAccentForeground)
	}
	button.Children(func() {
		ui.Icon(view, icon).FontSize(16)
		ui.Text(view, label).FontSize(14).SingleLine()
	}).OnClick(func() { state.Open(section) })
}
