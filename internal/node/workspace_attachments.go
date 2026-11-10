package node

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (node *Node) capturedAttachment(ctx context.Context, turn protocol.TurnID, id protocol.AttachmentID) (protocol.Attachment, bool, error) {
	var attachment protocol.Attachment
	var current bool
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM turn_attachments a JOIN agent_runs r ON r.turn=a.turn WHERE a.attachment=?2 AND (a.turn=?1 OR json_extract(r.body,'$.joined')=?1))", turn.String(), id.String()).Scan(&current); err != nil {
			return err
		}
		var err error
		attachment, err = store.ResolveTurnAttachment(ctx, db, turn, id)
		return err
	})
	return attachment, current, err
}
