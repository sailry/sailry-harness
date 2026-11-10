package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// toolCheckpointDetail derives a single successful edit's display from its
// captured bytes, independent of later working-tree changes or display recipes.
func toolCheckpointDetail(ctx context.Context, db *sql.Conn, turn protocol.TurnID, raw *rawAgentTool, detail *protocol.ToolDetail) error {
	if raw.ID == nil || raw.Result == nil || raw.Failure != nil || detail.Resolved == nil || detail.Resolved.Output == nil {
		return nil
	}
	var body []byte
	var before sql.NullString
	var after string
	err := db.QueryRowContext(ctx, `SELECT c.body,c.before_text,c.after_text FROM file_checkpoints c
 JOIN agent_approvals a ON a.id=c.approval WHERE a.turn=?1 AND a.call=?2`, turn.String(), *raw.ID).Scan(&body, &before, &after)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	var saved checkpointRecord
	if err = json.Unmarshal(body, &saved); err != nil {
		return err
	}
	if saved.Set || strings.EqualFold(path.Ext(saved.Path), ".ipynb") {
		return nil
	}
	name := saved.Path
	if raw.Display != nil && raw.Display.Output != nil && raw.Display.Output.Body != nil && raw.Display.Output.Body.Path != nil {
		if value := agentString(agentReference(*raw.Display.Output.Body.Path, agentJSON(raw.Arguments), raw.value(), nil)); value != nil && *value != "" {
			name = *value
		}
	}
	text, added, removed := files.TextDiff(name, before.String, after)
	if added == 0 && removed == 0 || text == "" {
		return nil
	}
	if len(text) > protocol.MAX_DIFF_BYTES {
		var bounded strings.Builder
		for _, line := range strings.SplitAfter(text, "\n") {
			if bounded.Len()+len(line) > protocol.MAX_DIFF_BYTES {
				break
			}
			bounded.WriteString(line)
		}
		text = bounded.String()
	}
	format := protocol.ToolDisplayProjectionFormatUnified
	output := detail.Resolved.Output
	output.Body = &protocol.ToolDetailBody{Text: text, Path: &name, Diff: &format}
	output.Preview = ""
	output.Notices = nil
	return nil
}
