package node

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ScopedKnowledge runs workspace requests and receipt inspection on the
// execution Node that owns the collection.
func (node *Node) ScopedKnowledge(ctx context.Context, caller protocol.NodeID, call protocol.ScopedKnowledgeCall) (link.Admission, error) {
	body, err := json.Marshal(call)
	if err != nil || len(body) > protocol.MAX_FRAME_BYTES || json.Unmarshal(body, &call) != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid scoped knowledge request"}
	}
	if err := call.Validate(); err != nil {
		return link.Admission{}, err
	}
	original := call.Request
	if call.Original != nil {
		original = *call.Original
	}
	if original.Version != protocol.VERSION {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "Node or protocol version mismatch"}
	}
	if err := protocol.ValidateKnowledgeExecution(original, node.ID()); err != nil {
		return link.Admission{}, err
	}
	verified, err := node.authorizeCommand(ctx, caller, original)
	if err != nil {
		return link.Admission{}, err
	}
	if err := node.validatePluginRequest(verified, caller, original); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if original.Plugin.Worktree != nil {
		// Access checks let the local owner address missing resources. Verify
		// the captured worktree still exists before using it as a scope.
		if err := node.profile.Store.Node.Read(verified, func(db *sql.Conn) error {
			_, err := store.ReadWorkspaceRoot(verified, db, *original.Plugin.Worktree, false)
			return err
		}); err != nil {
			return link.Admission{}, nodeFault(err)
		}
	}
	return node.routeKnowledge(verified, caller, protocol.ResourceCall{Request: call.Request, Original: call.Original}, false)
}
