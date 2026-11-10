package host

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func clone[T any](value T) T {
	body, _ := json.Marshal(value)
	var result T
	_ = json.Unmarshal(body, &result)
	return result
}

func unavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func unknown(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: message}
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
