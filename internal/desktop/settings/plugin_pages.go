package settings

import (
	"cmp"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// NativePage contributes a body to the standard settings page frame.
type NativePage struct {
	Section    Section
	Group      string
	Icon       *ui.SVG
	Order      uint16
	Render     func(*ui.Context, *State)
	BindWindow func(host any) WindowBinding
}

type PluginPage struct {
	Section     Section
	Label       string
	Icon        *ui.SVG
	Group       string
	Order       uint16
	ContentOnly bool
	Render      func(*ui.Context)
}

// ManagedNodeSelect renders the same setting target on built-in and mounted
// settings pages.
func ManagedNodeSelect(c *ui.Context, m *Model) bool {
	return managedNodeSelect(c, m, &m.ManagedNode)
}

func (m *Model) packageTargetOffline() bool {
	if node := m.managedNodeID(); node != "" {
		return !slices.ContainsFunc(m.managedHosts(), func(host ProviderHost) bool {
			return host.Node == node && host.Connected
		})
	}
	return false
}

func (section Section) navigationOrder() uint16 {
	switch section {
	case General, Permissions:
		return 100
	case Plugins:
		return 160
	case Appearance:
		return 200
	case Backup, Conversation:
		return 300
	case Shortcuts:
		return 400
	case Dictation:
		return 500
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

// SetNativePages replaces the native settings contributions.
func (state *State) SetNativePages(pages []NativePage) {
	previous := state.HasSection(state.Section)
	state.nativePages = slices.Clone(pages)
	if previous && !state.HasSection(state.Section) {
		state.Open(General)
	}
}

// NativePageState returns the window's state for a native page, creating it on first use.
func (state *State) NativePageState(section Section, create func() any) any {
	if value, ok := state.nativeState[section]; ok {
		return value
	}
	value := create()
	if state.nativeState == nil {
		state.nativeState = make(map[Section]any)
	}
	state.nativeState[section] = value
	return value
}

func (state *State) SetPluginPages(pages []PluginPage) {
	previous := state.HasSection(state.Section)
	state.pluginPages = slices.Clone(pages)
	if previous && !state.HasSection(state.Section) {
		state.Open(Plugins)
	}
}

// PluginPages returns the plugin pages available in settings navigation and search.
func (state *State) PluginPages() []PluginPage {
	pages := slices.Clone(state.pluginPages)
	return slices.DeleteFunc(pages, func(page PluginPage) bool { return page.ContentOnly })
}

// HasSection reports whether the section can be opened, including content-only plugin pages.
func (state *State) HasSection(section Section) bool {
	_, _, ok := state.lookupSection(section)
	return ok
}

func (state *State) HasNativePage(section Section) bool {
	page, _, _ := state.lookupSection(section)
	return page != nil
}

func (state *State) lookupSection(section Section) (*NativePage, *PluginPage, bool) {
	if section == "" {
		return nil, nil, false
	}
	if builtInSection(section) {
		return nil, nil, true
	}
	if i := slices.IndexFunc(state.nativePages, func(page NativePage) bool { return page.Section == section }); i >= 0 {
		return &state.nativePages[i], nil, true
	}
	if i := slices.IndexFunc(state.pluginPages, func(page PluginPage) bool { return page.Section == section }); i >= 0 {
		return nil, &state.pluginPages[i], true
	}
	return nil, nil, false
}

func (state *State) activePage() (Section, *NativePage, *PluginPage) {
	native, plugin, ok := state.lookupSection(state.Section)
	if !ok {
		return General, nil, nil
	}
	return state.Section, native, plugin
}

func builtInSection(section Section) bool {
	for _, group := range Groups {
		if slices.ContainsFunc(group.Items, func(item Item) bool { return item.Section == section }) {
			return true
		}
	}
	return false
}

// ContributedPages returns the localized navigation and search entries. Open and Page handle rendering.
func (state *State) ContributedPages(c *ui.Context) []PluginPage {
	pages := make([]PluginPage, 0, len(state.nativePages)+len(state.pluginPages))
	seen := make(map[Section]bool, cap(pages))
	add := func(page PluginPage) {
		if page.Section == "" || seen[page.Section] || builtInSection(page.Section) {
			return
		}
		seen[page.Section] = true
		if page.ContentOnly {
			return
		}
		for _, group := range Groups {
			if group.Title == page.Group {
				pages = append(pages, page)
				return
			}
		}
	}
	for _, page := range state.nativePages {
		add(PluginPage{
			Section: page.Section, Label: i18n.T(c, string(page.Section)), Icon: page.Icon,
			Group: page.Group, Order: page.Order,
		})
	}
	for _, page := range state.pluginPages {
		add(page)
	}
	return pages
}

func settingsNavigation(view *ui.Context, state *State, page PluginPage, selected bool) {
	palette := theme.Of(view)
	button := ui.ButtonBase(view.Key(string(page.Section))).Label(page.Label).Height(28).PaddingX(8).Gap(8).Radius(palette.Radius).
		Justify(ui.Start).TextColor(palette.SidebarForeground)
	button.Background(palette.SidebarItem(selected, button.Hovered())).AccessSelected(selected)
	if selected {
		button.TextColor(palette.SidebarAccentForeground)
	}
	button.Children(func() {
		ui.Icon(view, page.Icon).FontSize(16)
		ui.Text(view, page.Label).FontSize(14).SingleLine()
		if page.Section == About && state.Updater.Attention() {
			ui.Spacer(view)
			ui.Box(view.Key("update")).Size(6, 6).Radius(3).Background(palette.Danger).Shrink(0)
		}
	}).OnClick(func() { state.Open(page.Section) })
}
