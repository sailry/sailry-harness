package live

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// gitHistoryMore reads the page after the ones the history lists.
func (p *workspaceProject) gitHistoryMore() {
	if p.loading["git-log"] || len(p.gitLogPages) == 0 {
		return
	}
	if cursor := p.gitLogPages[len(p.gitLogPages)-1].Next; cursor != nil {
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
				p.gitLogPages = nil
			}
			p.gitLogPages = append(p.gitLogPages, page)
		}
		p.projectGitHistory(false)
	})
}

// projectGitHistory lists the pages read as one history, newest first: the
// graph draws a line only between commits of one list, so a page of its own
// would cut the lines that cross into it. A commit a later page repeats,
// as when the repository gained one between the reads, is listed once.
func (p *workspaceProject) projectGitHistory(loading bool) {
	state := panel.GitHistoryState{Loading: loading}
	if len(p.gitLogPages) > 0 {
		state.Next = p.gitLogPages[len(p.gitLogPages)-1].Next != nil
		p.data.Repo.Commits = nil
		listed := map[string]bool{}
		for _, page := range p.gitLogPages {
			state.Partial = state.Partial || page.ReferencesTruncated
			for _, entry := range page.Entries {
				state.Partial = state.Partial || entry.Truncated
				if listed[entry.ID] {
					continue
				}
				listed[entry.ID] = true
				commit := viewmodel.Commit{ID: entry.ID, Message: entry.Message, Author: entry.Author, Refs: entry.References, Parents: entry.Parents}
				if entry.Timestamp != 0 {
					commit.Time = time.Unix(entry.Timestamp, 0)
				}
				p.data.Repo.Commits = append(p.data.Repo.Commits, commit)
			}
		}
	}
	p.panel.SetGitHistory(state)
}
