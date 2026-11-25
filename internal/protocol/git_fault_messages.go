package protocol

// These fault messages distinguish branch conflicts that share a wire error code.
const (
	GitBranchExists     = "branch already exists"
	GitBranchInWorktree = "branch belongs to another worktree"
)
