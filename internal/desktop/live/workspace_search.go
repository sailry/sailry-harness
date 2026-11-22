package live

import (
	"context"
	"errors"
	"log/slog"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (p *workspaceProject) cancelFileSearch() {
	p.searchGeneration++
	if p.searchCancel != nil {
		p.searchCancel()
		p.searchCancel = nil
	}
}
func workspaceSearchOptions(query panel.SearchQuery) protocol.FileSearch {
	globs := []string{}
	for _, line := range strings.Split(query.Filter, "\n") {
		if value := strings.TrimSpace(line); value != "" {
			globs = append(globs, value)
		}
	}
	return protocol.FileSearch{Query: query.Query, Regex: query.Regex, CaseSensitive: query.CaseSensitive, Globs: globs, Mode: protocol.SearchModeContent, Limit: 200}
}
func (p *workspaceProject) searchFiles(query panel.SearchQuery, done func(panel.SearchResult)) {
	p.cancelFileSearch()
	generation := p.searchGeneration
	ctx, cancel := context.WithCancel(p.runtime.ctx)
	p.searchCancel = cancel
	command := protocol.Command{Kind: "search_files", Data: protocol.CommandSearchFilesPayload{Worktree: p.owner.ID, Options: workspaceSearchOptions(query)}}
	p.runtime.async(func() {
		out, err := p.runtime.controller.Do(ctx, command)
		p.runtime.enqueue(func(c *ui.Context) {
			if generation != p.searchGeneration {
				return
			}
			p.searchCancel = nil
			cancel()
			if err != nil {
				if !errors.Is(err, context.Canceled) {
					key := "file_search_failed"
					if faultCode(err) == protocol.ErrorCodeInvalidRequest {
						key = "file_search_invalid"
					} else {
						kit.RecordFailure("desktop", command.Kind, kit.Failed, err, slog.String("stage", "file_search"))
					}
					kit.Toast(c, kit.Failed, i18n.T(c, key))
				}
				done(panel.SearchResult{Failed: true})
				return
			}
			value, ok := detail[protocol.SearchResults](out.Data)
			if !ok {
				done(panel.SearchResult{Failed: true})
				return
			}
			result := panel.SearchResult{Partial: value.Truncated || value.Partial || value.Skipped > 0}
			for _, match := range value.Matches {
				if match.Matched {
					result.Matches = append(result.Matches, panel.SearchMatch{Path: match.Path, Line: int(match.LineNumber), Text: match.Line})
				}
			}
			done(result)
		})
	})
}
