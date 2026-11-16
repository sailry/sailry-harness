package node

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// ScopedKnowledge validates a UI plugin against its execution Node before
// forwarding its exact operation through the user's captured controller.
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
	if selected := knowledgeTarget(original); selected != (protocol.NodeID{}) && selected != original.Target {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "knowledge request must target its storage Node"}
	}
	verified, err := node.authorizeCommand(ctx, caller, original)
	if err != nil {
		return link.Admission{}, err
	}
	if err := node.validatePluginRequest(verified, caller, original); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if original.Plugin.Worktree != nil {
		// Access checks let the local owner address missing resources so their
		// service can report NotFound. A scope captured for forwarding must
		// actually belong here before its provenance is trusted elsewhere.
		if err := node.profile.Store.Node.Read(verified, func(db *sql.Conn) error {
			_, err := store.ReadWorkspaceRoot(verified, db, *original.Plugin.Worktree, false)
			return err
		}); err != nil {
			return link.Admission{}, nodeFault(err)
		}
	}
	scope, err := node.captureResourceScope(verified, original)
	if err != nil {
		return link.Admission{}, err
	}
	var controller protocol.NodeID
	err = node.profile.Store.Node.Read(verified, func(db *sql.Conn) error {
		var err error
		controller, err = store.CurrentController(verified, db, node.ID())
		return err
	})
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	resource := protocol.ResourceCall{Request: call.Request, Original: call.Original, Scope: scope}
	local := original.Target == node.ID() && original.Command.Kind != "query_knowledge" && original.Command.Kind != "list_knowledge_collections"
	if query, valid := dispatchPayload[protocol.KnowledgeQuery](original.Command.Data); valid && protocol.KnowledgeQueryLocal(query, node.ID()) {
		local = true
	}
	if controller != node.ID() && !local {
		return node.sendResource(verified, controller, resource, false)
	}
	return node.routeKnowledge(verified, caller, resource, !local)
}
