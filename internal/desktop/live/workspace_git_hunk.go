package live

import (
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type workspaceGitDiff struct {
	diff   protocol.GitDiff
	status protocol.GitStatus
}

func workspaceManualHunkIndex(displayed viewmodel.Hunk, patch string) int {
	wanted := workspaceHunkFingerprint(displayed)
	matched := -1
	for index, hunk := range workspaceHunks(patch) {
		if workspaceHunkFingerprint(hunk) == wanted {
			if matched >= 0 {
				return -1
			}
			matched = index
		}
	}
	return matched
}
func (p *workspaceProject) manualGitHunk(c *ui.Context, relative string, displayed viewmodel.Hunk, action string) {
	scope := p.panel.Git.Scope()
	if scope == "" {
		scope = "all"
	}
	key := scope + "/" + relative
	captured, exists := p.manualDiffs[key]
	selected := workspaceManualHunkIndex(displayed, captured.diff.Text)
	if !exists || !p.diffs[key] || selected < 0 || captured.diff.Binary || captured.diff.Truncated || captured.status.IndexRevision == nil || scope == "staged" || action != "stage" && action != "reject" {
		p.workspaceError(c, protocol.ErrorCodeRevisionConflict, "complete working-tree hunk source is not loaded")
		return
	}
	readKey := "manual-hunk/source/" + relative
	delete(p.failedReads, readKey)
	p.read(readKey, protocol.Command{Kind: "read_file", Data: protocol.CommandReadFilePayload{Worktree: p.owner.ID, Path: relative}}, func(c *ui.Context, out protocol.Output) {
		source, ok := detail[protocol.FileContent](out.Data)
		if !ok || source.Revision == nil || source.Truncated {
			p.workspaceError(c, protocol.ErrorCodeRevisionConflict, "complete working file is unavailable for hunk review")
			return
		}
		digest := blake3.Sum256([]byte(captured.diff.Text))
		input := protocol.CommandApplyGitHunkPayload{Worktree: p.owner.ID, Path: relative, Scope: protocol.GitDiffScope(scope), Operation: protocol.GitHunkOperation(action), Hunk: uint32(selected), DiffRevision: fmt.Sprintf("%x", digest), ExpectedRevision: *source.Revision, ExpectedHead: captured.status.Head, ExpectedBranch: captured.status.Branch, ExpectedIndex: *captured.status.IndexRevision}
		kind := gitIndex
		if action != "stage" {
			kind = gitOperation
		}
		p.mutateNamed(kind, protocol.Command{Kind: "apply_git_hunk", Data: input}, func(c *ui.Context, out protocol.Output) {
			if _, ok := detail[protocol.GitHunkApplied](out.Data); out.Kind == "git_hunk_applied" && ok {
				p.refresh()
				kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
			}
		})
	})
}
