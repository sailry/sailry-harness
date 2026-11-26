// Package desktop selects the modules shipped with Harness.
package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	browser "github.com/sailry/sailry-harness/modules/browser/desktop"
	computer "github.com/sailry/sailry-harness/modules/computer/desktop"
	connections "github.com/sailry/sailry-harness/modules/connections/desktop"
	connectors "github.com/sailry/sailry-harness/modules/connectors/desktop"
	decision "github.com/sailry/sailry-harness/modules/decision/desktop"
	hooks "github.com/sailry/sailry-harness/modules/hooks/desktop"
	knowledge "github.com/sailry/sailry-harness/modules/knowledge/desktop"
	market "github.com/sailry/sailry-harness/modules/market/desktop"
	media "github.com/sailry/sailry-harness/modules/media/desktop"
	memory "github.com/sailry/sailry-harness/modules/memory/desktop"
	ocr "github.com/sailry/sailry-harness/modules/ocr/desktop"
	providers "github.com/sailry/sailry-harness/modules/providers/desktop"
	runtime "github.com/sailry/sailry-harness/modules/runtime/desktop"
	skills "github.com/sailry/sailry-harness/modules/skills/desktop"
	teams "github.com/sailry/sailry-harness/modules/teams/desktop"
	websearch "github.com/sailry/sailry-harness/modules/web-search/desktop"
)

// HarnessModules returns the ordered desktop modules shipped with Harness.
func HarnessModules() []shell.Module {
	return []shell.Module{
		market.Register,
		providers.Register, connectors.Register, teams.Register, skills.Register,
		knowledge.Register, memory.Register, media.Register, decision.Register,
		websearch.Register, browser.Register, computer.Register,
		connections.Register, hooks.Register, runtime.Register, ocr.Register,
	}
}

// Harness returns the module contributions for an official Desktop window.
func Harness() shell.Assembly {
	return shell.Assemble(HarnessModules()...)
}
