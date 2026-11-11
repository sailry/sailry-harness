package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func workspaceGitDocumentFile(diff protocol.GitDiff) panel.GitDocumentFile {
	return panel.GitDocumentFile{
		FileChange: viewmodel.FileChange{Path: diff.Path, Added: int(diff.Additions), Removed: int(diff.Deletions), Hunks: workspaceHunks(diff.Text)},
		Binary:     diff.Binary, Truncated: diff.Truncated,
	}
}

func (p *workspaceProject) showGitDocument(id string) {
	// Captured reads may finish after navigation; keep their own project cache
	// without selecting a different worktree's panel.
	if p.runtime.workspace.selected == p.owner.ID {
		p.showItem(panel.Changes, id)
	}
}

func (p *workspaceProject) openGitCommit(id string) {
	key := "git-document/commit/" + id
	delete(p.failedReads, key)
	p.read(key, protocol.Command{Kind: "read_git_commit", Data: protocol.CommandReadGitCommitPayload{Worktree: p.owner.ID, Commit: id}}, func(_ *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.GitCommit](out.Data)
		if out.Kind != "git_commit" || !ok {
			return
		}
		files := make([]panel.GitDocumentFile, 0, len(value.Files))
		for _, file := range value.Files {
			files = append(files, workspaceGitDocumentFile(file))
		}
		title := value.Entry.ID[:min(8, len(value.Entry.ID))]
		p.showGitDocument(p.panel.SetGitCommit(id, title, value.Entry.Message, files, value.Truncated))
	})
}

func (p *workspaceProject) openGitStash(id string) {
	key := "git-document/stash/" + id
	delete(p.failedReads, key)
	p.read(key, protocol.Command{Kind: "read_git_stash", Data: protocol.CommandReadGitStashPayload{Worktree: p.owner.ID, Commit: id}}, func(c *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.GitDiff](out.Data)
		if out.Kind != "git_diff" || !ok {
			return
		}
		p.showGitDocument(p.panel.SetGitDiff(id, i18n.T(c, "git_stash_menu"), workspaceGitDocumentFile(value)))
	})
}

func (p *workspaceProject) openGitOutput() {
	key := "git-document/output"
	delete(p.failedReads, key)
	p.read(key, protocol.Command{Kind: "read_git_output", Data: protocol.CommandReadGitOutputPayload{Worktree: p.owner.ID}}, func(c *ui.Context, out protocol.Output) {
		value, ok := detail[protocol.OutputGitOutputPayload](out.Data)
		if out.Kind != "git_output" || !ok {
			return
		}
		p.showGitDocument(p.panel.SetGitText("output", i18n.T(c, "git_output"), value.Text))
	})
}
