// Package settings is the settings navigation and its pages: the App, AI,
// Tools and System groups of Section::GROUPS. Each page renders its part of
// the view model (model.go) over preview data; docs/settings.md lists them.
package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Section is a settings page, named by its title's i18n key
// (Section::key).
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
	Decision     Section = "settings_decision"
	Terminal     Section = "settings_terminal"
	Connections  Section = "settings_connections"
	Hooks        Section = "hooks_settings"
	Runtime      Section = "settings_runtime"
	OCR          Section = "settings_ocr"
	Permissions  Section = "settings_permissions"
	Backup       Section = "settings_backup"
	About        Section = "settings_about"
)

// Item is a section in the navigation with its icon (Section::icon).
type Item struct {
	Section Section
	Icon    *ui.SVG
}

// Groups is the navigation of Section::GROUPS, in its order and icons.
var Groups = []struct {
	Title string
	Items []Item
}{
	{"settings_group_app", []Item{
		{General, icons.Settings}, {Appearance, icons.Palette}, {Conversation, icons.Chat},
		{Shortcuts, icons.ALargeSmall}, {Dictation, icons.ALargeSmall},
	}},
	{"settings_group_ai", []Item{
		{Providers, icons.Cpu}, {Connectors, icons.Plug}, {Teams, icons.Users}, {Plugins, icons.Gallery},
		{Skills, icons.Bot}, {Knowledge, icons.BookOpen}, {Decision, icons.BrainCircuit},
	}},
	{"settings_group_tools", []Item{
		{Terminal, icons.Terminal}, {Connections, icons.Network}, {Hooks, icons.Settings2},
		{Runtime, icons.Cpu}, {OCR, icons.ALargeSmall},
	}},
	{"settings_group_system", []Item{
		{Permissions, icons.Settings2}, {Backup, icons.Upload}, {About, icons.Info},
	}},
}

// State is the settings' part of the app state: the section shown, the
// providers' search and the pages' view model.
type State struct {
	Section Section
	Query   string // the providers' search
	// Details is the extension details page shown in the right column, nil for
	// none.
	Details     *Detail
	pluginPages []PluginPage
	*Model
}

// New opens on General, as Workspace does, over the preview's settings.
func New() *State { return &State{Section: General, Model: NewModel(preview.SettingsData())} }

// NewLive has controller defaults and no preview fixtures or simulated loads.
func NewLive() *State {
	return &State{Section: General, Model: &Model{Live: true, Keys: NewKeys(), Toast: 1,
		Notify: [4]bool{true, true, true, true}, TextSize: 14, FontSize: 14, PasteProtection: true}}
}

// Nav builds the settings navigation in place of the sessions.
func Nav(c *ui.Context, s *State) {
	p := theme.Of(c)
	ui.Scroll(c.Key("settings-nav")).Grow(1).Padding(8, 12).Children(func() {
		for _, g := range Groups {
			ui.Column(c.Key(g.Title)).Gap(2).Margin(0, 0, 12, 0).Children(func() {
				ui.Text(c, i18n.T(c, g.Title)).FontSize(14).Height(32).PaddingX(8).AlignSelf(ui.Stretch).
					TextColor(p.MutedForeground.Alpha(0.7)).SingleLine()
				for _, entry := range settingsGroupNavigation(c, g.Title, g.Items, s.pluginPages) {
					settingsNavigation(c, s, entry.Section, entry.Label, entry.Icon)
				}
			})
		}
	})
}

// Open shows a section, leaving the one shown as Workspace's park does:
// a shortcut being recorded is canceled.
func (s *State) Open(sec Section) {
	s.Keys.Cancel()
	s.Section, s.Details = sec, nil
}

// Title is the header's title for the section.
func Title(c *ui.Context, s *State) string {
	for _, page := range s.pluginPages {
		if page.Section == s.Section {
			return page.Label
		}
	}
	return i18n.T(c, string(s.Section))
}

// Page builds the section's page in the settings frame: its title and
// description over its groups (SettingsPage).
func Page(c *ui.Context, s *State, d *preview.Data, look Look) {
	for _, page := range s.pluginPages {
		if page.Section == s.Section {
			if page.Render != nil {
				page.Render(c)
			} else {
				kit.Result(c, kit.ResultProps{Title: i18n.T(c, "unavailable"), Card: true})
			}
			return
		}
	}
	look.Live = s.Live
	look.Actions = s.AppearanceActions
	desc := string(s.Section) + "_description"
	if i18n.In("en", desc) == desc {
		desc = ""
	} else {
		desc = i18n.T(c, desc)
	}
	kit.Page(c, s.Section, i18n.T(c, string(s.Section)), desc, func() {
		if s.Live && s.Section != General && s.Section != Appearance && s.Section != Conversation && s.Section != Shortcuts && s.Section != Providers && s.Section != Hooks && s.Section != Connections && s.Section != Plugins && s.Section != Skills && s.Section != Runtime && s.Section != Connectors && s.Section != Terminal && s.Section != Dictation && s.Section != OCR && s.Section != Knowledge && s.Section != Decision && s.Section != Permissions && s.Section != Backup && s.Section != About {
			kit.Result(c, kit.ResultProps{Title: i18n.T(c, "unavailable"), Card: true})
			return
		}
		switch s.Section {
		case General:
			general(c, s.Model)
		case Appearance:
			appearance(c, look)
		case Conversation:
			conversation(c, s.Model)
		case Shortcuts:
			shortcuts(c, &s.Keys)
		case Dictation:
			dictation(c, s.Model)
		case Providers:
			providers(c, s)
		case Connectors:
			connectors(c, s.Model)
		case Teams:
			teams(c, s.Model)
		case Plugins:
			plugins(c, s)
		case Skills:
			skills(c, s)
		case Terminal:
			terminal(c, s.Model)
		case Connections:
			connections(c, s.Model)
		case Hooks:
			hooks(c, s.Model)
		case Runtime:
			runtimePage(c, s.Model)
		case OCR:
			localModels(c, s.Model, &s.OCR, "runtime_ocr", "runtime_ocr_model", "runtime_ocr_enable", "search_driver_auto")
		case Knowledge:
			localModels(c, s.Model, &s.Knowledge, "knowledge_settings", "knowledge_default_model", "", "knowledge_keyword_only")
		case Decision:
			localModels(c, s.Model, &s.Decision, "runtime_decision", "runtime_decision_model", "runtime_decision_enable", "local_models_none")
		case Permissions:
			permissions(c, s.Model)
		case Backup:
			backup(c, s.Model)
		case About:
			about(c, s.Model)
		default:
			// The AI pages still to come (phase 5b).
			kit.Result(c, kit.ResultProps{Icon: icons.Inbox, Title: i18n.T(c, "settings_empty"), Card: true})
		}
		libraryDialog(c, s.Model)
	})
}
