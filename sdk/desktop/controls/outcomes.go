package controls

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

type Outcome = kit.Outcome

const Done = kit.Done

func Toast(c *ui.Context, outcome Outcome, message string) { kit.Toast(c, outcome, message) }
