// Package settings implements the settings navigation, page frame and shared view model.
package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Section identifies a settings page. For native pages it is the title's i18n key.
type Section string

const (
	General      Section = "settings_general"
	Appearance   Section = "settings_appearance"
	Conversation Section = "settings_conversation"
	Shortcuts    Section = "settings_shortcuts"
	Dictation    Section = "settings_dictation"
	Providers    Section = "settings_providers"
	Connectors   Section = "settings_connectors"
	Teams        Section = "settings_teams"
	Plugins      Section = "settings_plugins"
	Skills       Section = "settings_skills"
	Knowledge    Section = "settings_knowledge"
	Memory       Section = "settings_memory"
	Media        Section = "settings_media"
	Decision     Section = "settings_decision"
	Terminal     Section = "settings_terminal"
	Search       Section = "settings_search"
	Browser      Section = "settings_browser"
	Computer     Section = "settings_computer"
	Connections  Section = "settings_connections"
	Hooks        Section = "hooks_settings"
	Runtime      Section = "settings_runtime"
	OCR          Section = "settings_ocr"
	Permissions  Section = "settings_permissions"
	Backup       Section = "settings_backup"
	About        Section = "settings_about"
	// Experimental holds the development and debugging settings.
	Experimental Section = "settings_experimental"
)

// Item is a section in the navigation with its icon (Section::icon).
type Item struct {
	Section Section
	Icon    *ui.SVG
}

// Groups lists built-in items and the group order shared by all settings pages.
var Groups = []struct {
	Title string
	Items []Item
}{
	{"settings_group_app", []Item{
		{General, icons.Settings}, {Appearance, icons.Palette}, {Conversation, icons.Chat},
		{Shortcuts, icons.ALargeSmall}, {Dictation, icons.ALargeSmall},
	}},
	{"settings_group_ai", []Item{
		{Plugins, icons.Gallery},
	}},
	{"settings_group_tools", nil},
	{"settings_group_system", []Item{
		{Permissions, icons.Settings2}, {Backup, icons.Upload}, {Experimental, icons.Code}, {About, icons.Info},
	}},
}

// State holds the current settings section and the window's settings view model.
type State struct {
	Section Section
	Query   string // the providers' search
	// Details is the extension details page shown in the right column, nil for
	// none.
	Details     *Detail
	nativePages []NativePage
	nativeState map[Section]any
	pluginPages []PluginPage
	*Model
}

// New opens on General, as Workspace does, over the preview's settings.
func New() *State { return &State{Section: General, Model: NewModel(preview.SettingsData())} }

// NewLive has controller defaults and no preview fixtures or simulated loads.
func NewLive() *State {
	return &State{Section: General, Model: &Model{Live: true, Keys: NewKeys(), Toast: 1,
		Notify: [4]bool{true, true, true, true}, TextSize: DefaultTextSize, SessionOrder: 1, FontSize: 14, PasteProtection: true}}
}

// Nav builds the settings navigation in place of the sessions.
func Nav(c *ui.Context, s *State) {
	p := theme.Of(c)
	active, _, _ := s.activePage()
	pages := s.ContributedPages(c)
	ui.Scroll(c.Key("settings-nav")).Grow(1).Padding(8, 12).Children(func() {
		for _, g := range Groups {
			entries := settingsGroupNavigation(c, g.Title, g.Items, pages)
			if len(entries) == 0 {
				continue
			}
			ui.Column(c.Key(g.Title)).Gap(2).Margin(0, 0, 12, 0).Children(func() {
				ui.Text(c, i18n.T(c, g.Title)).FontSize(14).Height(32).PaddingX(8).AlignSelf(ui.Stretch).
					TextColor(p.MutedForeground.Alpha(0.7)).SingleLine()
				for _, entry := range entries {
					settingsNavigation(c, s, entry, entry.Section == active)
				}
			})
		}
	})
}

// Open shows a section, or General if it is unavailable, and cancels shortcut recording.
func (s *State) Open(sec Section) {
	if !s.HasSection(sec) {
		sec = General
	}
	if s.Model != nil {
		s.Keys.Cancel()
	}
	s.Section, s.Details = sec, nil
}

// Title is the header's title for the section.
func Title(c *ui.Context, s *State) string {
	section, _, plugin := s.activePage()
	if plugin != nil {
		return plugin.Label
	}
	return i18n.T(c, string(section))
}

// Page builds the section's page in the settings frame: its title and
// description over its groups (SettingsPage).
func Page(c *ui.Context, s *State, d *preview.Data, look Look) {
	section, native, plugin := s.activePage()
	if section != s.Section {
		s.Open(section)
	}
	if plugin != nil {
		if plugin.Render != nil {
			plugin.Render(c)
		} else {
			settingsTargetStatus(c, "unavailable")
		}
		return
	}
	descKey := string(section) + "_description"
	desc := i18n.T(c, descKey)
	if desc == descKey {
		desc = ""
	}
	// Each section's page starts at its top; the model pages' Available
	// tab loads the hub's next page as it nears the end (moreLibrary).
	if s.scrolled != section {
		s.scrolled, s.pageScroll = section, ui.ScrollState{}
	}
	// The library is open while a page's Available tab shows this frame.
	s.library.open = false
	targetChanged := false
	var headerActions func()
	if section.ManagesNode() || native != nil && native.BindWindow != nil {
		headerActions = func() {
			targetChanged = managedNodeSelect(c, s.Model, &s.ManagedNode)
		}
	}
	kit.Page(c, section, i18n.T(c, string(section)), desc, func() {
		if targetChanged {
			return
		}
		switch {
		case native == nil:
			builtInPage(c, s, section, look)
		case native.Render != nil:
			native.Render(c, s)
		default:
			settingsTargetStatus(c, "unavailable")
		}
		libraryDialogs(c, s.Model)
	}, headerActions).TrackScroll(&s.pageScroll)
}

func builtInPage(c *ui.Context, s *State, section Section, look Look) {
	switch section {
	case General:
		general(c, s.Model)
	case Appearance:
		look.Live, look.Busy = s.Live, s.Busy[BusyThemes]
		look.Actions = s.AppearanceActions
		appearance(c, look, s.Model)
	case Conversation:
		conversation(c, s.Model)
	case Shortcuts:
		shortcuts(c, &s.Keys)
	case Dictation:
		dictation(c, s.Model)
	case Plugins:
		plugins(c, s)
	case Permissions:
		permissions(c, s.Model)
	case Backup:
		backup(c, s.Model)
	case About:
		about(c, s.Model)
	case Experimental:
		experimental(c, s.Model)
	}
}
