package plugins

import (
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *Host) primeRepositoryLocation() {
	scope := owner.scope()
	if owner.native.RepositoryLocation == nil || owner.binding.Client == nil || scope.Worktree == nil ||
		(scope.Surface.Kind != "composer" && scope.Surface.Kind != "project") ||
		!slices.Contains(owner.binding.Actions, protocol.PluginActionReadGit) {
		return
	}
	// Location controls may wait for repository metadata before issuing their first SDK read.
	go func() {
		_, _ = owner.read(owner.ctx, protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: *scope.Worktree}})
	}()
}

func (owner *Host) publishRepositoryLocation(scope protocol.PluginContext, output protocol.Output) {
	publish := owner.native.RepositoryLocation
	if publish == nil || scope.Worktree == nil || output.Kind != "git_status" || owner.ctx.Err() != nil {
		return
	}
	status, ok := protocol.DataAs[protocol.GitStatus](output.Data)
	if !ok {
		return
	}
	branch := ""
	if status.Branch != nil {
		branch = *status.Branch
	} else if status.Head != nil {
		branch = (*status.Head)[:min(7, len(*status.Head))]
	}
	publish(*scope.Worktree, status.Kind, branch)
}
