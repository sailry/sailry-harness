package plugins

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ComposerStatistics(statistics *protocol.ConversationStatistics) (json.RawMessage, error) {
	body, err := json.Marshal(statistics)
	if err != nil {
		return nil, err
	}
	value, err := decodeHostJSON(body)
	if err != nil {
		return nil, err
	}
	return json.Marshal(exactHostUsage(value))
}
