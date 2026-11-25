package testassembly

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestTerminalTranslations(t *testing.T) {
	catalog := i18n.New(Locales)
	if value := i18n.In("en", "terminal_environment"); value != "terminal_environment" {
		t.Fatalf("fixture copy leaked into the shared catalog: %q", value)
	}
	for _, language := range []string{"en", "zh-CN"} {
		t.Run(language, func(t *testing.T) {
			state := settings.New()
			state.Host.Draft.Env = nil
			state.Shells = []preview.Shell{{}}
			state.Font = ""
			tester := ui.NewTesterWith(func(view *ui.Context) {
				i18n.Bind(view, catalog)
				theme.Apply(view, "", theme.Dark)
				terminal(view, state)
			}, 900, 1100, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			tester.SetLocale(language)
			tester.Frame()
			for _, key := range []string{"terminal_environment", "terminal_shell_group", "terminal_shell", "terminal_shell_default", "terminal_input", "settings_paste_protection", "settings_paste_description", "settings_terminal_display", "settings_font_size", "settings_font_family", "settings_font_default"} {
				translated := catalog.In(language, key)
				if translated == key || !tester.HasText(translated) || tester.HasText(key) {
					t.Errorf("%s did not display %s as %q: %q", language, key, translated, tester.Texts())
				}
			}
			editor := state.TerminalEnvEditor()
			*editor = settings.TerminalEnvEditor{Open: true, Index: -1}
			tester.Frame()
			if title := catalog.In(language, "terminal_environment_add"); !tester.HasText(title) {
				t.Fatalf("dialog title is not translated: %q", tester.Texts())
			}
			for _, key := range []string{"terminal_variable", "terminal_value"} {
				label := catalog.In(language, key)
				if _, found := tester.AccessByRole(ui.RoleTextField, label); label == key || !found {
					t.Errorf("%s dialog has no translated %s field: %q", language, key, tester.Texts())
				}
			}
		})
	}
}
