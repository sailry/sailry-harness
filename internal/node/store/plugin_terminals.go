package store

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func checkPluginTerminalRequest(ctx context.Context, db *sql.Conn, request protocol.Request) error {
	scope := request.Plugin
	var target struct {
		Worktree *protocol.WorktreeID     `json:"worktree"`
		Terminal *protocol.TerminalID     `json:"terminal"`
		Launch   *protocol.TerminalLaunch `json:"launch"`
	}
	body, err := json.Marshal(request.Command.Data)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(body, &target); err != nil {
		return err
	}
	if request.Command.Kind == "create_terminal" {
		target.Launch = new(protocol.TerminalLaunch)
		if err := json.Unmarshal(body, target.Launch); err != nil {
			return err
		}
	}
	desktop := scope.Turn == nil && scope.Invocation == nil &&
		(scope.Surface.Kind == "workspace" || scope.Surface.Kind == "project")
	worktree := target.Worktree
	if target.Launch != nil {
		worktree = &target.Launch.Worktree
		if target.Launch.Access == protocol.ProcessAccessUnrestricted && !desktop {
			return fault(protocol.ErrorCodePermissionDenied, "plugins cannot create unrestricted terminals")
		}
	}
	if target.Terminal != nil {
		info, err := ReadTerminal(ctx, db, *target.Terminal)
		if err != nil {
			return err
		}
		if worktree != nil && (info.Worktree == nil || *worktree != *info.Worktree) {
			return fault(protocol.ErrorCodePermissionDenied, "terminal belongs to another worktree")
		}
		if info.Access != nil && *info.Access == protocol.ProcessAccessUnrestricted &&
			(request.Command.Kind == "input_terminal" || request.Command.Kind == "open_terminal" && !desktop) {
			return fault(protocol.ErrorCodePermissionDenied, "plugins cannot execute in unrestricted terminals")
		}
		worktree = info.Worktree
	}
	if worktree == nil || scope.Worktree == nil || *worktree != *scope.Worktree {
		return fault(protocol.ErrorCodePermissionDenied, "plugin terminal is outside the captured resource scope")
	}
	return nil
}
