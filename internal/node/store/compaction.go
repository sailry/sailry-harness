package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ReadCompaction(ctx context.Context, db *sql.Conn) (protocol.ConversationCompactionSettings, error) {
	value, err := readBody[protocol.ConversationCompactionSettings](ctx, db, "SELECT body FROM compaction_settings WHERE singleton=1")
	if err != nil {
		return protocol.ConversationCompactionSettings{}, err
	}
	if value == nil {
		return protocol.DefaultConversationCompactionSettings(), nil
	}
	if value.Percent < 1 || value.Percent > 99 {
		return *value, invalid("compaction percentage is outside the supported range")
	}
	return *value, nil
}
func saveCompaction(ctx context.Context, db *sql.Conn, input protocol.ConversationCompactionSettings) (protocol.Output, []protocol.Event, error) {
	if input.Percent < 1 || input.Percent > 99 {
		return protocol.Output{}, nil, invalid("compaction percentage is outside the supported range")
	}
	current, err := ReadCompaction(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if err = checkRevision(current.Revision, input.Revision); err != nil {
		return protocol.Output{}, nil, err
	}
	input.Revision, err = nextRevision(input.Revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	body, err := encode(input)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO compaction_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return protocol.Output{Kind: "compaction_settings", Data: input}, []protocol.Event{{Kind: "compaction_settings_changed", Data: protocol.EventCompactionSettingsChangedPayload{Revision: input.Revision}}}, err
}
