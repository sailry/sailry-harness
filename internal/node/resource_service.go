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
	var controller protocol.NodeID
	err = node.profile.Store.Node.Read(verified, func(db *sql.Conn) error {
		var err error
		controller, err = store.CurrentController(verified, db, node.ID())
		return err
	})
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	scope, err := node.captureResourceScope(verified, request)
	if err != nil {
		return link.Admission{}, err
	}
	selected := knowledgeTarget(request)
	if selected != (protocol.NodeID{}) && selected != request.Target {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "knowledge request must target its storage Node"}
	}
	// Local resources remain usable without the controller. A query with no
	// selected Node asks the controller's complete view while it is reachable.
	localResource := request.Target == node.ID() && request.Command.Kind != "query_knowledge" && request.Command.Kind != "list_knowledge_collections"
	if controller != node.ID() && !localResource && selected != node.ID() && request.Plugin != nil {
		return node.sendResource(verified, controller, protocol.ResourceCall{Request: request, Scope: scope}, false)
	}
	aggregate := request.Command.Kind == "list_knowledge_collections" || request.Command.Kind == "query_knowledge"
	return node.routeKnowledge(verified, caller, protocol.ResourceCall{Request: request, Scope: scope}, aggregate)
}

func knowledgeTarget(request protocol.Request) protocol.NodeID {
	var selected *protocol.NodeID
	switch input := request.Command.Data.(type) {
	case *protocol.CommandCreateKnowledgeCollectionPayload:
		selected = input.Node
		if selected == nil {
			selected = store.ReadKnowledgeSource(input.Definition.Source).Node
		}
	case *protocol.CommandUpdateKnowledgeCollectionPayload:
		selected = input.Node
		if selected == nil {
			selected = store.ReadKnowledgeSource(input.Definition.Source).Node
		}
	case *protocol.CommandDeleteKnowledgeCollectionPayload:
		selected = input.Node
	case *protocol.CommandRebuildKnowledgeCollectionPayload:
		selected = input.Node
	case *protocol.CommandPauseKnowledgeCollectionPayload:
		selected = input.Node
	case *protocol.KnowledgeCitationRequest:
		if input.Node != (protocol.NodeID{}) {
			return input.Node
		}
	}
	if selected != nil {
		return *selected
	}
	return protocol.NodeID{}
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

// Resource is the only captured runtime entry point. A controller can send a
// local operation to its storage host; an execution host can ask its controller
// to resolve a resource. The latter can traverse only one direct host edge.
func (node *Node) Resource(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall) (link.Admission, error) {
	if err := call.Validate(); err != nil {
		return link.Admission{}, err
	}
	if node.network == nil || call.Request.Version != protocol.VERSION {
		return link.Admission{}, permissionDenied()
	}
	relations, err := node.network.Relationships(ctx)
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	index := slices.IndexFunc(relations, func(peer link.PeerRelationship) bool { return peer.ID == caller && peer.Direct && !peer.Removed })
	if index < 0 {
		return link.Admission{}, permissionDenied()
	}
	inbound := call.Local
	if inbound && !relations[index].DirectDirection.AllowsInbound() || !inbound && !relations[index].DirectDirection.AllowsOutbound() {
		return link.Admission{}, permissionDenied()
	}
	original := call.Request
	if call.Original != nil {
		original = *call.Original
	}
	if original.Plugin == nil && call.Scope != nil || original.Plugin != nil && (call.Scope == nil || !reflect.DeepEqual(original.Plugin, call.Scope.Origin)) {
		return link.Admission{}, permissionDenied()
	}
	if !inbound && (call.Scope == nil || call.Scope.Origin == nil || call.Scope.Origin.Turn == nil && call.Scope.Origin.Invocation == nil) {
		return link.Admission{}, permissionDenied()
	}
	var actor protocol.User
	err = node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		if inbound {
			actor, err = store.CallerUser(ctx, db, node.ID(), caller)
		} else {
			actor, err = store.LocalUser(ctx, db)
		}
		return err
	})
	if err != nil || actor.State != "active" {
		return link.Admission{}, permissionDenied()
	}
	verified, err := store.WithKnowledgeScope(store.WithUser(ctx, actor), call.Scope)
	if err != nil {
		return link.Admission{}, err
	}
	if err := store.CheckKnowledgeScope(verified, original); err != nil {
		return link.Admission{}, err
	}
	if original.Command.Kind == "read_knowledge_candidates" {
		query, valid := original.Command.Data.(*protocol.PluginStorageSearch)
		if !valid || query.Scope != "" && query.Scope != protocol.PluginStorageScopeNode {
			return link.Admission{}, permissionDenied()
		}
	}
	if inbound && call.Request.Target != node.ID() {
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeWrongTarget, Message: "resource call cannot forward through a storage Node"}
	}
	return node.routeKnowledge(verified, caller, call, !inbound)
}

func (node *Node) routeKnowledge(ctx context.Context, caller protocol.NodeID, call protocol.ResourceCall, aggregate bool) (link.Admission, error) {
	request := call.Request
	if request.Target != node.ID() && !aggregate {
		return node.sendResource(ctx, request.Target, call, true)
	}
	if request.Target != node.ID() && request.Command.Kind != "query_knowledge" && request.Command.Kind != "list_knowledge_collections" {
		return node.sendResource(ctx, request.Target, call, true)
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

func (node *Node) sendResource(ctx context.Context, target protocol.NodeID, call protocol.ResourceCall, outbound bool) (link.Admission, error) {
	call.Local = outbound
	if outbound {
		if remote := node.controllerClients()[target]; remote != nil {
			if !remote.Connected() {
				return link.Admission{}, resourceUnavailable()
			}
			return remote.Resource(ctx, call)
		}
	}
	if node.network == nil {
		return link.Admission{}, resourceUnavailable()
	}
	relations, err := node.network.Relationships(ctx)
	if err != nil {
		return link.Admission{}, nodeFault(err)
	}
	if !slices.ContainsFunc(relations, func(peer link.PeerRelationship) bool {
		return peer.ID == target && peer.Direct && !peer.Removed && (outbound && peer.DirectDirection.AllowsOutbound() || !outbound && peer.DirectDirection.AllowsInbound())
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
	input := call.Request.Command.Data.(*protocol.CommandInspectRequestPayload)
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
