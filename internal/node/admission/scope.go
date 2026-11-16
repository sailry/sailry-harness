package admission

import (
	"context"
	"encoding/json"
	"reflect"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type userKey struct{}

// WithUser retains only the authenticated actor for ledger outcome recovery.
func WithUser(ctx context.Context, user protocol.UserID) context.Context {
	return context.WithValue(ctx, userKey{}, user)
}

type Resource struct {
	Kind string `json:"kind"`
	ID   string `json:"id"`
}

// Scope contains no authored text, settings, configuration or credentials.
type Scope struct {
	Kind      string           `json:"kind"`
	User      *protocol.UserID `json:"user,omitempty"`
	Remote    bool             `json:"remote,omitempty"`
	Resources []Resource       `json:"resources,omitempty"`
}

func scopeBody(ctx context.Context, request protocol.Request) []byte {
	value := Scope{Kind: request.Command.Kind}
	if user, ok := ctx.Value(userKey{}).(protocol.UserID); ok {
		value.User = &user
	}
	if request.Command.Kind == "call_host_handler" {
		// The handler's origin may be another execution Node. Its typed
		// resource IDs are provenance, not resources in this ledger.
		value.Remote = true
		body, _ := json.Marshal(value)
		return body
	}
	if command, ok := request.Command.Data.(*protocol.CommandCreateSessionAtPayload); ok {
		value.Remote = command.Target != request.Target
	}
	if !value.Remote {
		scopeResources(reflect.ValueOf(request.Command.Data), &value.Resources)
	}
	scopeResources(reflect.ValueOf(request.Plugin), &value.Resources)
	body, _ := json.Marshal(value)
	return body
}
func scopeResources(value reflect.Value, resources *[]Resource) {
	if !value.IsValid() {
		return
	}
	for value.Kind() == reflect.Pointer || value.Kind() == reflect.Interface {
		if value.IsNil() {
			return
		}
		value = value.Elem()
	}
	if value.CanInterface() {
		var resource Resource
		switch id := value.Interface().(type) {
		case protocol.ProjectID:
			resource = Resource{"project", id.String()}
		case protocol.WorktreeID:
			resource = Resource{"worktree", id.String()}
		case protocol.SessionID:
			resource = Resource{"session", id.String()}
		case protocol.TerminalID:
			resource = Resource{"terminal", id.String()}
		case protocol.TurnID:
			resource = Resource{"turn", id.String()}
		case protocol.ArtifactID:
			resource = Resource{"artifact", id.String()}
		case protocol.NotificationID:
			resource = Resource{"notification", id.String()}
		case protocol.ApprovalID:
			resource = Resource{"approval", id.String()}
		case protocol.QuestionID:
			resource = Resource{"question", id.String()}
		case protocol.CheckpointID:
			resource = Resource{"checkpoint", id.String()}
		case protocol.AttachmentID:
			resource = Resource{"attachment", id.String()}
		case protocol.ConversationInput:
			return
		}
		if resource.Kind != "" {
			*resources = append(*resources, resource)
			return
		}
	}
	switch value.Kind() {
	case reflect.Struct:
		for i := 0; i < value.NumField(); i++ {
			if value.Type().Field(i).IsExported() {
				scopeResources(value.Field(i), resources)
			}
		}
	case reflect.Slice, reflect.Array:
		for i := 0; i < value.Len(); i++ {
			scopeResources(value.Index(i), resources)
		}
	case reflect.Map:
		iter := value.MapRange()
		for iter.Next() {
			scopeResources(iter.Value(), resources)
		}
	}
}
