package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (p *workspaceProject) gitHistoryPage(next bool) {
	if p.loading["git-log"] || len(p.gitLogPages) == 0 {
		return
	}
	if !next {
		if p.gitLogPage > 0 {
			p.gitLogPage--
			p.projectGitHistory(false)
		}
		return
	}
	if p.gitLogPage+1 < len(p.gitLogPages) {
		p.gitLogPage++
		p.projectGitHistory(false)
		return
	}
	if cursor := p.gitLogPages[p.gitLogPage].Next; cursor != nil {
		p.readGitLog(cursor, false)
	}
}
func (p *workspaceProject) readGitLog(cursor *protocol.GitLogCursor, reset bool) {
	if p.loading["git-log"] {
		return
	}
	delete(p.failedReads, "git-log")
	p.projectGitHistory(true)
	p.read("git-log", protocol.Command{Kind: "read_git_log", Data: protocol.CommandReadGitLogPayload{Worktree: p.owner.ID, Limit: 100, Cursor: cursor}}, func(c *ui.Context, out protocol.Output) {
		if page, ok := detail[protocol.GitLog](out.Data); ok {
			if reset {
				p.gitLogPages = []protocol.GitLog{page}
				p.gitLogPage = 0
			} else {
				p.gitLogPages = append(p.gitLogPages, page)
				p.gitLogPage = len(p.gitLogPages) - 1
			}
		}
		p.projectGitHistory(false)
		_ = c
	})
}
func (p *workspaceProject) projectGitHistory(loading bool) {
	state := panel.GitHistoryState{Loading: loading}
	if len(p.gitLogPages) > 0 {
		page := p.gitLogPages[p.gitLogPage]
		state.Offset, state.Next, state.Previous, state.Partial = page.Offset, page.Next != nil, p.gitLogPage > 0, page.ReferencesTruncated
		p.data.Repo.Commits = nil
		for _, entry := range page.Entries {
			state.Partial = state.Partial || entry.Truncated
			p.data.Repo.Commits = append(p.data.Repo.Commits, viewmodel.Commit{ID: entry.ID, Message: entry.Message, Author: entry.Author, Refs: entry.References})
		}
	}
	p.panel.SetGitHistory(state)
}
