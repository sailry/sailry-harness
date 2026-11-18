package node

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// admitResourceCommand captures the caller before selecting a data entry. The
// selected storage Node remains in the request sent to the final service.
func (node *Node) admitResourceCommand(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if request.Version != protocol.VERSION || request.Target != node.ID() && caller != node.ID() {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "Node or protocol version mismatch"}
	}
	body, err := json.Marshal(request)
	if err != nil || len(body) > protocol.MAX_FRAME_BYTES {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid resource request"}
	}
	if err := json.Unmarshal(body, &request); err != nil {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid resource request"}
	}
	verified, err := node.authorizeCommand(ctx, caller, request)
	if err != nil {
		return link.Admission{}, err
	}
	if err := node.validatePluginRequest(verified, caller, request); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if request.Command.Kind == "read_resource_catalog" {
		return node.resourceReadAdmission(verified, request, func(ctx context.Context) (protocol.Output, error) {
			catalog, err := node.resourceCatalog(ctx)
			return protocol.Output{Kind: "resource_catalog", Data: catalog}, err
		})
	}
	selected := knowledgeTarget(request)
	if selected != (protocol.NodeID{}) && selected != request.Target {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "knowledge request must target its storage Node"}
	}
	if executionKnowledgeOnly(verified, request.Plugin) {
		if err := protocol.ValidateKnowledgeExecution(request, node.ID()); err != nil {
			return link.Admission{}, err
		}
		return node.admitKnowledgeLocal(verified, caller, request)
	}
	scope, err := node.captureResourceScope(verified, request)
	if err != nil {
		return link.Admission{}, err
	}
	return node.routeKnowledge(verified, caller, protocol.ResourceCall{Request: request, Scope: scope}, true)
}

func executionKnowledgeScope(scope *protocol.PluginContext) bool {
	return scope != nil && (scope.Worktree != nil || scope.Session != nil || scope.Turn != nil || scope.Invocation != nil)
}

func executionKnowledgeOnly(ctx context.Context, scope *protocol.PluginContext) bool {
	user, _ := store.UserFromContext(ctx)
	captured, _ := ctx.Value(capturedDispatchKey{}).(bool)
	return captured || !user.Local || executionKnowledgeScope(scope)
}

func knowledgeTarget(request protocol.Request) protocol.NodeID {
	return protocol.KnowledgeTarget(request.Command)
}

func (node *Node) captureResourceScope(ctx context.Context, request protocol.Request) (*protocol.ResourceScope, error) {
	if request.Plugin == nil {
		return nil, nil
	}
	var info protocol.PluginInfo
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		info, err = store.ResolvePluginInfo(ctx, db, request.Plugin.Package)
		return err
	})
	if err != nil {
		return nil, nodeFault(err)
	}
	return &protocol.ResourceScope{Plugin: &info, Origin: request.Plugin}, nil
}

// Resource admits management requests at their storage Node from a direct
// controller.
func (node *Node) Resource(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (link.Admission, error) {
	if err := call.Validate(); err != nil {
		return link.Admission{}, err
	}
	if caller == node.ID() {
		return node.localResource(ctx, call)
	}
	if !call.Local || node.network == nil || call.Request.Version != protocol.VERSION {
		return link.Admission{}, permissionDenied()
	}
	relations, err := node.network.Relationships(ctx)
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if !slices.ContainsFunc(relations, func(peer link.PeerRelationship) bool {
		return peer.ID == caller && peer.Direct && !peer.Removed && peer.DirectDirection.AllowsInbound()
	}) {
		return link.Admission{}, permissionDenied()
	}
	original := call.Request
	if call.Original != nil {
		original = *call.Original
	}
	if original.Plugin == nil && call.Scope != nil || original.Plugin != nil && (call.Scope == nil || !reflect.DeepEqual(original.Plugin, call.Scope.Origin)) {
		return link.Admission{}, permissionDenied()
	}
	var actor protocol.User
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		actor, err = store.CallerUser(ctx, db, node.ID(), caller)
		return err
	})
	if err != nil || actor.State != "active" {
		return link.Admission{}, permissionDenied()
	}
	if call.Request.Target != node.ID() {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "resource call cannot forward through a storage Node"}
	}
	if executionKnowledgeScope(original.Plugin) {
		return link.Admission{}, permissionDenied()
	}
	if original.Command.Kind == "call_host_handler" {
		return node.routeHostHandler(store.WithUser(ctx, actor), caller, call)
	}
	if selected := knowledgeTarget(original); selected != (protocol.NodeID{}) && selected != node.ID() {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "knowledge request must target its storage Node"}
	}
	verified, err := store.WithKnowledgeScope(store.WithUser(ctx, actor), call.Scope)
	if err != nil {
		return link.Admission{}, err
	}
	if err := store.CheckKnowledgeScope(verified, original); err != nil {
		return link.Admission{}, err
	}
	return node.routeKnowledge(verified, caller, call, false)
}

func (node *Node) localResource(ctx context.Context, call protocol.ResourceCall) (link.Admission, error) {
	original := call.Request
	if call.Original != nil {
		original = *call.Original
	}
	if original.Version != protocol.VERSION || call.Scope != nil {
		return link.Admission{}, permissionDenied()
	}
	verified, err := node.authorizeCommand(ctx, node.ID(), original)
	if err != nil {
		return link.Admission{}, err
	}
	if err := node.validatePluginRequest(verified, node.ID(), original); err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if selected := knowledgeTarget(original); selected != (protocol.NodeID{}) && selected != original.Target {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "knowledge request must target its storage Node"}
	}
	call.Scope, err = node.captureResourceScope(verified, original)
	if err != nil {
		return link.Admission{}, err
	}
	if original.Command.Kind == "call_host_handler" {
		if original.Target != node.ID() && executionKnowledgeOnly(verified, original.Plugin) {
			return link.Admission{}, permissionDenied()
		}
		return node.routeHostHandler(verified, node.ID(), call)
	}
	aggregate := !executionKnowledgeOnly(verified, original.Plugin)
	if !aggregate {
		if err := protocol.ValidateKnowledgeExecution(original, node.ID()); err != nil {
			return link.Admission{}, err
		}
	}
	return node.routeKnowledge(verified, node.ID(), call, aggregate)
}

func (node *Node) routeKnowledge(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall, aggregate bool) (link.Admission, error) {
	request := call.Request
	if request.Target != node.ID() {
		return node.sendResource(ctx, request.Target, call)
	}
	if request.Command.Kind == "inspect_request" {
		return node.resourceReadAdmission(ctx, request, func(ctx context.Context) (protocol.Output, error) {
			return node.readKnowledgeOutcome(ctx, caller, call)
		})
	}
	if aggregate && (request.Command.Kind == "list_knowledge_collections" || request.Command.Kind == "query_knowledge") {
		return node.resourceReadAdmission(ctx, request, func(ctx context.Context) (protocol.Output, error) {
			return node.aggregateKnowledge(ctx, caller, call)
		})
	}
	return node.admitKnowledgeLocal(ctx, caller, request)
}

func (node *Node) sendResource(ctx context.Context, target protocol.NodeID, call protocol.ResourceCall) (link.Admission, error) {
	call.Local = true
	if remote := node.controllerClients()[target]; remote != nil {
		if !remote.Connected() {
			return link.Admission{}, resourceUnavailable()
		}
		return remote.Resource(ctx, call)
	}
	if node.network == nil {
		return link.Admission{}, resourceUnavailable()
	}
	relations, err := node.network.Relationships(ctx)
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if !slices.ContainsFunc(relations, func(peer link.PeerRelationship) bool {
		return peer.ID == target && peer.Direct && !peer.Removed && peer.DirectDirection.AllowsOutbound()
	}) {
		return link.Admission{}, permissionDenied()
	}
	addresses, err := node.network.Peers(ctx)
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	for _, address := range addresses {
		if address.ID == target {
			return node.network.Remote(address).Resource(ctx, call)
		}
	}
	return link.Admission{}, resourceUnavailable()
}

func (node *Node) resourceReadAdmission(ctx context.Context, request protocol.Request, read func(context.Context) (protocol.Output, error)) (link.Admission, error) {
	if err := node.begin(); err != nil {
		return link.Admission{}, err
	}
	completion := make(chan link.Response, 1)
	go func() {
		defer node.work.Done()
		defer close(completion)
		operation, cancel := context.WithCancel(ctx)
		defer cancel()
		defer context.AfterFunc(node.ctx, cancel)()
		completion <- response(read(operation))
	}()
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: false}, Completion: completion}, nil
}

func resourceUnavailable() *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "knowledge source Node is unavailable"}
}

func resourceFault(err error) *protocol.Fault {
	if err == nil {
		return nil
	}
	var direct *protocol.Fault
	if errors.As(err, &direct) {
		return direct
	}
	var wrapped *link.FaultError
	if errors.As(err, &wrapped) {
		return &wrapped.Fault
	}
	return resourceUnavailable()
}

// readKnowledgeOutcome inspects exactly the target's original request. It does
// not reissue effects and never interprets remote plugin Turn IDs as local IDs.
func (node *Node) readKnowledgeOutcome(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (protocol.Output, error) {
	input, valid := dispatchPayload[protocol.CommandInspectRequestPayload](call.Request.Command.Data)
	if !valid {
		return protocol.Output{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid resource inspection"}
	}
	var outcome protocol.RequestOutcome
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		outcome, err = admission.Inspect(ctx, db, caller, input.ID, input.Digest)
		if err != nil || outcome.Kind != "completed" {
			return err
		}
		var principal []byte
		if err := db.QueryRowContext(ctx, "SELECT scope FROM requests WHERE caller=?1 AND id=?2", caller[:], input.ID.String()).Scan(&principal); err != nil {
			return err
		}
		var captured admission.Scope
		if err := json.Unmarshal(principal, &captured); err != nil {
			return err
		}
		user, err := store.CurrentUser(ctx, db)
		if err != nil {
			return err
		}
		if captured.User == nil {
			if !user.Local {
				outcome = deniedOutcome()
			}
			return nil
		}
		owner, err := store.ResolveUser(ctx, db, *captured.User)
		if err != nil || owner.ID != user.ID {
			outcome = deniedOutcome()
			return err
		}
		var output *protocol.Output
		switch completed := outcome.Data.(type) {
		case protocol.Result[protocol.Output, protocol.Fault]:
			output = completed.Ok
		case *protocol.Result[protocol.Output, protocol.Fault]:
			output = completed.Ok
		}
		if output != nil && call.Original != nil {
			if err := store.CheckKnowledgeOutcome(ctx, db, *call.Original, *output); err != nil {
				outcome = deniedOutcome()
			}
		}
		return nil
	})
	return protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: outcome}}, nodeFault(err)
}
