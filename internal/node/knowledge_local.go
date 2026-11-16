package node

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type knowledgeLocalKey struct{}

// KnowledgeLocal executes only this Node's knowledge service. Controller
// aggregation must use this entry point rather than route a request recursively.
func (node *Node) KnowledgeLocal(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	verified, err := node.authorizeCommand(ctx, caller, request)
	if err != nil {
		return protocol.Output{}, err
	}
	if err := node.validatePluginRequest(verified, caller, request); err != nil {
		return protocol.Output{}, nodeFault(err)
	}
	return node.knowledgeLocal(verified, caller, request)
}

// knowledgeLocal accepts only an already verified actor. ResourceCall validates
// the trusted execution scope at its entry point; remote Turn and Session IDs
// are not records in this Node's store and must not be resolved there again.
func (node *Node) knowledgeLocal(ctx context.Context, caller protocol.NodeID, request protocol.Request) (protocol.Output, error) {
	if user, present := store.UserFromContext(ctx); !present || user.State != "active" {
		return protocol.Output{}, permissionDenied()
	}
	if err := store.CheckKnowledgeScope(ctx, request); err != nil {
		return protocol.Output{}, err
	}
	if request.Command.Durable() {
		var err error
		err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			storage, err := store.CapturedUser(ctx, db)
			if err == nil {
				ctx = store.WithCapturedUser(ctx, storage)
			}
			return err
		})
		if err != nil {
			return protocol.Output{}, nodeFault(err)
		}
	}
	if request.Target != node.ID() || request.Version != protocol.VERSION {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "Node or protocol version mismatch"}
	}
	if !knowledgeCommand(request.Command.Kind) {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "knowledge command expected"}
	}
	body, err := json.Marshal(request)
	if err != nil || len(body) > protocol.MAX_FRAME_BYTES {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid knowledge request"}
	}
	if err := json.Unmarshal(body, &request); err != nil {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid knowledge request"}
	}
	ctx = context.WithValue(ctx, knowledgeLocalKey{}, true)
	admitted, err := node.dispatchKnowledge(ctx, caller, request, body)
	if err != nil {
		return protocol.Output{}, err
	}
	select {
	case result := <-admitted.Completion:
		if result.Fault != nil {
			return protocol.Output{}, result.Fault
		}
		return result.Output, nil
	case <-ctx.Done():
		code := protocol.ErrorCodeCancelled
		if request.Command.Durable() {
			code = protocol.ErrorCodeOutcomeUnknown
		}
		return protocol.Output{}, &protocol.Fault{Code: code, Message: "knowledge operation was cancelled"}
	}
}
