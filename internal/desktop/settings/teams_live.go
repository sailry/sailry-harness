package settings

import (
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// TeamPageState keeps presentation state with this settings model and lets the
// host discard Node-bound dialogs without owning their drafts.
type TeamPageState interface {
	CloseDialogs()
}

// TeamActions connects existing catalog editors to revision-checked writes.
type TeamActions struct {
	SaveConcurrency func()
	SaveTeam        func(viewmodel.Team, func(kit.Outcome))
	SaveRole        func(viewmodel.Role, func(kit.Outcome))
	RemoveTeam      func(viewmodel.Team, func(kit.Outcome))
	RemoveRole      func(viewmodel.Role, func(kit.Outcome))
}
