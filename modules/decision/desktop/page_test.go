package desktop

import (
	"fmt"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestDecisionSettingsCanClearMissingModel(t *testing.T) {
	const lang, missing = "zh-CN", "Previously selected model"
	assembly := shell.Assemble(Register)
	catalog := i18n.New(assembly.Locales...)
	for _, unavailable := range []bool{false, true} {
		t.Run(fmt.Sprintf("unavailable=%t", unavailable), func(t *testing.T) {
			page := settings.NewLive()
			page.SetNativePages(assembly.SettingsPages)
			page.Open(settings.Decision)
			page.Decision = settings.LocalModels{Kind: settings.ModelsDecision, Model: 0, Missing: missing, Unavailable: unavailable}
			var saves []int
			page.ModelActions = &settings.ModelActions{Save: func(key string) {
				if key != settings.ModelsDecision {
					t.Fatalf("saved %q from the Decision page", key)
				}
				saves = append(saves, page.Decision.Model)
			}}
			view := ui.NewTester(func(c *ui.Context) {
				i18n.Bind(c, catalog)
				theme.Apply(c, "", theme.Light)
				settings.Page(c, page, nil, settings.Look{})
			}, 900, 700)
			view.SetLocale(lang)
			view.Frame()
			model, none := catalog.In(lang, "runtime_decision_model"), catalog.In(lang, "local_models_none")
			trigger, ok := view.AccessByRole(ui.RoleButton, model)
			if !ok || trigger.Disabled || trigger.Value != missing || view.HasText(catalog.In(lang, "local_models_status")) {
				t.Fatalf("Decision configuration with no installed model = %+v, labels %q", trigger, view.Texts())
			}
			view.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
			view.Frame()
			view.Advance(time.Second)
			option, ok := view.AccessByRole(ui.RoleOption, none)
			if !ok {
				t.Fatalf("the model dropdown did not offer %q: %q", none, view.Texts())
			}
			view.ClickAt(option.Bounds.X+option.Bounds.W/2, option.Bounds.Y+option.Bounds.H/2)
			view.Frame()
			if got, _ := view.AccessByRole(ui.RoleButton, model); got.Value != none || len(saves) != 1 || saves[0] != -1 {
				t.Fatalf("selecting None showed %q and saved %+v", got.Value, saves)
			}
			for _, status := range []string{"connections_offline", "permission_denied"} {
				page.LocalModelsStatus = status
				view.Frame()
				if _, ok := view.AccessByRole(ui.RoleButton, model); ok || !view.HasText(catalog.In(lang, status)) {
					t.Fatalf("Decision page with %q: %q", status, view.Texts())
				}
			}
		})
	}
}
