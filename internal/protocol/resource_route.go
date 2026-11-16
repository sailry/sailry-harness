package protocol

import (
	"encoding/hex"
	"encoding/json"

	"lukechampine.com/blake3"
)

// RouteKey distinguishes the identity used at an entry Node from the Node
// holding the resource. Resource records retain Target as their real Node ID.
type RouteKey struct {
	Entry  NodeID `json:"entry"`
	Target NodeID `json:"target"`
}

// ResourceCall carries an execution Node's captured resource operation back
// through its controller. Request.Target remains the resource's actual Node.
// The Link authenticates the execution peer; no user identity is accepted here.
type ResourceCall struct {
	Local    bool           `json:"local,omitempty"`
	Request  Request        `json:"request"`
	Original *Request       `json:"original,omitempty"`
	Scope    *ResourceScope `json:"scope,omitempty"`
}

func (value *ResourceCall) UnmarshalJSON(data []byte) error {
	type plain ResourceCall
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*value = ResourceCall(decoded)
	return nil
}

// Validate binds recovery to the exact captured resource request. An
// inspection cannot turn this limited channel into a general request lookup.
func (call ResourceCall) Validate() error {
	invalid := func() error {
		return &Fault{Code: ErrorCodeInvalidRequest, Message: "resource operation is not supported"}
	}
	if ResourceCommand(call.Request.Command.Kind) {
		if call.Original != nil {
			return invalid()
		}
		return nil
	}
	if call.Request.Command.Kind != "inspect_request" || call.Original == nil ||
		!ResourceCommand(call.Original.Command.Kind) || !call.Original.Command.Durable() ||
		call.Original.Target != call.Request.Target || call.Original.Version != call.Request.Version {
		return invalid()
	}
	var inspect CommandInspectRequestPayload
	switch value := call.Request.Command.Data.(type) {
	case CommandInspectRequestPayload:
		inspect = value
	case *CommandInspectRequestPayload:
		if value == nil {
			return invalid()
		}
		inspect = *value
	default:
		return invalid()
	}
	body, err := json.Marshal(call.Original)
	if err != nil {
		return invalid()
	}
	digest := blake3.Sum256(body)
	if inspect.ID != call.Original.ID || inspect.Digest != hex.EncodeToString(digest[:]) {
		return invalid()
	}
	return nil
}

// ResourceCommand limits the reverse execution channel to knowledge and a
// captured package's declared host handlers.
func ResourceCommand(kind string) bool {
	switch kind {
	case "list_knowledge_collections", "create_knowledge_collection",
		"update_knowledge_collection", "delete_knowledge_collection",
		"pause_knowledge_collection", "rebuild_knowledge_collection",
		"query_knowledge", "read_knowledge_citation", "read_knowledge_candidates", "call_host_handler":
		return true
	}
	return false
}
