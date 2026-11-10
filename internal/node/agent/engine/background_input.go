package engine

import (
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// bindBackground reserves evidence before its exact canonical input append.
// Store confirms delivery in that append transaction, never on this reservation.
func (r *Runtime) bindBackground(x *execution, record *canonical.Record) error {
	unlock := r.LockPublication()
	defer unlock()
	var text *string
	err := r.db.Write(x.ctx, func(db *sql.Conn) error {
		var events []protocol.Event
		var err error
		x.inv.Run, text, events, err = store.BindBackground(x.ctx, db, x.inv.Run.Turn)
		if err != nil {
			return err
		}
		_, err = store.RecordEvents(x.ctx, db, r.node, events)
		return err
	})
	if err != nil {
		return err
	}
	if text == nil {
		return nil
	}
	message := record.MutableMessage()
	index := uint64(len(message.Content))
	message.Content = append(message.Content, canonical.Text(index, *text))
	message.PartCount = uint64(len(message.Content))
	record.Metadata.Input.NoticePart = &index
	for _, notice := range x.inv.Run.Notices {
		record.Metadata.Input.NoticeIDs = append(record.Metadata.Input.NoticeIDs, notice.ID)
	}
	if x.inv.Run.Notice {
		source := canonical.InputNotice
		record.Metadata.Input.Source = &source
	}
	r.publish()
	return nil
}
