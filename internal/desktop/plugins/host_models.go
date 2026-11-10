package plugins

import (
	"context"
	"encoding/json"
)

func (owner *Host) modelOperation(name string) HostOperation {
	return func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		input, err := hostValue[map[string]json.RawMessage](arguments, 0)
		if err != nil {
			return nil, err
		}
		switch name {
		case "list":
			if nativeProp(input, "target", "") != "node" || nativeProp(input, "kind", "") == "speech" {
				return nil, hostInvalid("Node local model list required")
			}
			return owner.readData(ctx, "read_local_models", map[string]any{"kind": optionalRaw(input, "kind")})
		case "search":
			return owner.readData(ctx, "search_models", input)
		case "versions":
			return owner.readData(ctx, "read_model_versions", input)
		case "resolve":
			return owner.readData(ctx, "resolve_model", input)
		case "register":
			if nativeProp(input, "target", "") == "controller" {
				return owner.native.operation(ctx, "sailry/models", name, arguments)
			}
			if nativeProp(input, "target", "") != "node" {
				return nil, hostInvalid("invalid model registration target")
			}
			return owner.prepareData(ctx, "register_local_model", map[string]any{"definition": input["definition"]})
		case "prepare":
			if nativeProp(input, "target", "") == "controller" {
				return owner.native.operation(ctx, "sailry/models", name, arguments)
			}
			if nativeProp(input, "target", "") != "node" {
				return nil, hostInvalid("invalid model installation target")
			}
			return owner.prepareData(ctx, "install_local_model", map[string]any{"id": input["id"]})
		}
		return nil, hostUnavailable("model operation is unavailable")
	}
}
