package plugins

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ToolCommand(kind string, data json.RawMessage) (protocol.Command, error) {
	envelope, err := json.Marshal(map[string]any{"kind": kind, "data": data})
	if err != nil {
		return protocol.Command{}, err
	}
	value, err := decodeValue(envelope)
	if err != nil {
		return protocol.Command{}, err
	}
	if err := scriptCommandRevisions(value); err != nil {
		return protocol.Command{}, err
	}
	body, err := json.Marshal(value)
	if err != nil {
		return protocol.Command{}, err
	}
	var command protocol.Command
	if err := json.Unmarshal(body, &command); err != nil {
		return protocol.Command{}, scriptInvalid("invalid plugin tool operation")
	}
	return command, nil
}

func ToolOutput(output protocol.Output) any {
	return scriptOutput(output)
}
