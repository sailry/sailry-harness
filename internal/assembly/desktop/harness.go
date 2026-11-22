// Package desktop selects the modules shipped with Harness.
package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	connections "github.com/sailry/sailry-harness/modules/connections/desktop"
	connectors "github.com/sailry/sailry-harness/modules/connectors/desktop"
	decision "github.com/sailry/sailry-harness/modules/decision/desktop"
	files "github.com/sailry/sailry-harness/modules/files/desktop"
	git "github.com/sailry/sailry-harness/modules/git/desktop"
	hooks "github.com/sailry/sailry-harness/modules/hooks/desktop"
	knowledge "github.com/sailry/sailry-harness/modules/knowledge/desktop"
	market "github.com/sailry/sailry-harness/modules/market/desktop"
	media "github.com/sailry/sailry-harness/modules/media/desktop"
	ocr "github.com/sailry/sailry-harness/modules/ocr/desktop"
	providers "github.com/sailry/sailry-harness/modules/providers/desktop"
	runtime "github.com/sailry/sailry-harness/modules/runtime/desktop"
	skills "github.com/sailry/sailry-harness/modules/skills/desktop"
	teams "github.com/sailry/sailry-harness/modules/teams/desktop"
	terminal "github.com/sailry/sailry-harness/modules/terminal/desktop"
)

// Harness returns the module contributions for an official Desktop window.
func Harness() shell.Assembly {
	return shell.Assemble(
		files.Register, git.Register, market.Register,
		providers.Register, connectors.Register, teams.Register, skills.Register,
		knowledge.Module(), media.Register, decision.Register,
		terminal.Register, connections.Register, hooks.Register, runtime.Register, ocr.Register,
	)
}
