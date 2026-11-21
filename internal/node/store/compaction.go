package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type CompactionPolicy struct {
	Percent    uint8
	UseDefault bool
}

func (policy CompactionPolicy) WindowTrigger(contextLimit uint32) uint64 {
	return uint64(contextLimit) * uint64(policy.Percent) / 100
}

func readCompactionPolicy(ctx context.Context, db *sql.Conn) (CompactionPolicy, error) {
	settings, err := ReadCompaction(ctx, db)
	return CompactionPolicy{Percent: settings.Percent, UseDefault: settings.Revision == 0}, err
}

func ReadCompaction(ctx context.Context, db *sql.Conn) (protocol.ConversationCompactionSettings, error) {
	value, err := readPersonalSetting[protocol.ConversationCompactionSettings](ctx, db, "compaction")
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
	err = writePersonalSetting(ctx, db, "compaction", body)
	return protocol.Output{Kind: "compaction_settings", Data: input}, []protocol.Event{{Kind: "compaction_settings_changed", Data: protocol.EventCompactionSettingsChangedPayload{Revision: input.Revision}}}, err
}
