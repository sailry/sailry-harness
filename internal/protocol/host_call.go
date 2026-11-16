package protocol

import "strings"

// HostHandlerCall invokes one declared handler of the captured package. The
// request target is the execution Node, never a field supplied to the handler.
type HostHandlerCall struct {
	Handler  string `json:"handler"`
	Input    any    `json:"input"`
	ReadOnly bool   `json:"read_only,omitempty"`
}

func (value *HostHandlerCall) UnmarshalJSON(body []byte) error {
	type plain HostHandlerCall
	var decoded plain
	if err := DecodeObject(body, &decoded, []string{"handler", "input"}, false); err != nil {
		return err
	}
	if strings.TrimSpace(decoded.Handler) == "" {
		return &Fault{Code: ErrorCodeInvalidRequest, Message: "host handler is required"}
	}
	*value = HostHandlerCall(decoded)
	return nil
}

func init() {
	commandVariants["call_host_handler"] = func() any { return new(HostHandlerCall) }
}

func HostHandlerPayload(command Command) (HostHandlerCall, bool) {
	switch value := command.Data.(type) {
	case HostHandlerCall:
		return value, command.Kind == "call_host_handler"
	case *HostHandlerCall:
		if value != nil {
			return *value, command.Kind == "call_host_handler"
		}
	}
	return HostHandlerCall{}, false
}
