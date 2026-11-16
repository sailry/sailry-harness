package protocol

// ScopedKnowledgeCall sends a plugin operation to its execution Node for scope
// validation before the Node follows the user's controller to the real target.
// It cannot carry a user, controller or previously trusted ResourceScope.
type ScopedKnowledgeCall struct {
	Request  Request  `json:"request"`
	Original *Request `json:"original,omitempty"`
}

func (call *ScopedKnowledgeCall) UnmarshalJSON(data []byte) error {
	type plain ScopedKnowledgeCall
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"request"}, false); err != nil {
		return err
	}
	*call = ScopedKnowledgeCall(decoded)
	return nil
}

func (call ScopedKnowledgeCall) Validate() error {
	original := call.Request
	if call.Original != nil {
		original = *call.Original
	}
	if original.Plugin == nil || !KnowledgeCommand(original.Command.Kind) {
		return &Fault{Code: ErrorCodeInvalidRequest, Message: "scoped knowledge operation is required"}
	}
	return (ResourceCall{Request: call.Request, Original: call.Original}).Validate()
}
