package desktop

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

func Page() settings.NativePage {
	return settings.NativePage{
		Section: settings.Decision,
		Group:   "settings_group_ai",
		Icon:    icons.BrainCircuit,
		Order:   300,
		Render: func(c *ui.Context, s *settings.State) {
			settings.LocalModelLibrary(c, s.Model, &s.Decision, "runtime_decision", "runtime_decision_model", "runtime_decision_enable", "local_models_none")
		},
	}
}
