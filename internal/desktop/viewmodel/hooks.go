package viewmodel

// HookPhase is the presentation order of the three worktree hook phases.
type HookPhase int

const (
	BeforeTool HookPhase = iota
	AfterTool
	TurnEnd
)

type Hook struct {
	ID, Name, Command string
	Phase             HookPhase
	Tools             []string
	TimeoutMS         int
	Enabled           bool
}
